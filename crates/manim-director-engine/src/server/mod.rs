//! The local HTTP server behind the workbench (HTTP API v2): token-guarded
//! JSON routes, file streaming, and a resumable SSE stream.

mod auth;
mod error;
mod events;
mod feed;
mod files;
mod routes;
mod state;
mod watch;
mod workbench;

#[cfg(test)]
mod tests;

use crate::{shutdown_signal, Scheduler};
use anyhow::{anyhow, bail, Result};
use auth::Session;
use axum::{
    extract::{DefaultBodyLimit, Request},
    http::{header, HeaderValue},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Router,
};
use events::{EventHub, RING_CAPACITY};
use state::{AppState, REQUEST_BODY_BYTES, SOURCE_BODY_BYTES};
use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use workbench::Workbench;

/// Printed before serving with `--allow-remote`.
pub const REMOTE_WARNING: &str = "WARNING: --allow-remote: the API is reachable from other machines over plain HTTP. Anyone holding the session token can edit project files and run code as you.";

#[derive(Debug, Clone)]
pub struct ServeConfig {
    pub address: SocketAddr,
    pub workbench_dir: Option<PathBuf>,
    /// Accept any Host header and allow binding any address.
    pub allow_remote: bool,
}

/// A bound server; [`Server::url`] is the link that signs a browser in.
pub struct Server {
    listener: TcpListener,
    state: AppState,
}

impl Server {
    pub async fn bind(config: ServeConfig, scheduler: Scheduler) -> Result<Self> {
        // The Host check knows loopback only by these names, so a browser
        // could not sign in on any other address.
        let ip = config.address.ip();
        if ip != Ipv4Addr::LOCALHOST && ip != Ipv6Addr::LOCALHOST && !config.allow_remote {
            bail!("{ip} is not 127.0.0.1 or ::1; pass --allow-remote to serve another address");
        }
        let listener = TcpListener::bind(config.address)
            .await
            .map_err(|error| match error.kind() {
                io::ErrorKind::AddrInUse => anyhow!(
                    "port {} is in use, probably by a running engine: open the workbench link it printed, or pass --port",
                    config.address.port()
                ),
                _ => anyhow!("cannot listen on {}: {error}", config.address),
            })?;
        let port = listener.local_addr()?.port();
        let workbench = match config.workbench_dir.filter(|path| path.is_dir()) {
            Some(directory) => Workbench::Directory(directory),
            None => Workbench::Embedded,
        };
        let hub = Arc::new(EventHub::new(scheduler.instance_id(), RING_CAPACITY));
        let state = AppState::new(
            scheduler,
            Session::new(port)?,
            port,
            config.allow_remote,
            workbench,
            hub,
        );
        Ok(Self { listener, state })
    }

    pub fn address(&self) -> SocketAddr {
        self.listener.local_addr().expect("a bound listener")
    }

    /// The tokenized workbench URL on the bound address (loopback for a
    /// wildcard bind).
    pub fn url(&self) -> String {
        let address = self.address();
        let host = match address.ip() {
            IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
            ip => ip,
        };
        format!(
            "http://{}/?token={}",
            SocketAddr::new(host, address.port()),
            self.state.session.token()
        )
    }

    /// Serves until SIGINT/SIGTERM, then cancels this engine's jobs.
    pub async fn run(self) -> Result<()> {
        let Self { listener, state } = self;
        let closing = state.closing.clone();
        let jobs = state.scheduler.subscribe();
        let tasks = [
            tokio::spawn(feed::run(state.clone(), jobs, closing.clone())),
            tokio::spawn(watch::index(state.clone(), closing.clone())),
            tokio::spawn(watch::files(state.clone(), closing.clone())),
            tokio::spawn(watch::catalog(state.clone(), closing.clone())),
        ];
        tokio::spawn(watch::doctor(state.clone()));
        let signalled = closing.clone();
        let served = axum::serve(listener, router(state.clone()))
            .with_graceful_shutdown(async move {
                shutdown_signal().await;
                // Ends the SSE streams too, so graceful shutdown can finish.
                signalled.cancel();
            })
            .await;
        closing.cancel();
        for task in tasks {
            let _ = task.await;
        }
        state.scheduler.shutdown().await;
        Ok(served?)
    }
}

fn router(state: AppState) -> Router {
    let routes = Router::new()
        .route("/api/health", get(routes::health))
        .route("/api/state", get(routes::workspace))
        .route("/api/jobs", get(routes::list_jobs).post(routes::submit_job))
        .route("/api/jobs/{id}", get(routes::get_job))
        .route("/api/jobs/{id}/cancel", post(routes::cancel_job))
        .route("/api/jobs/{id}/logs", get(routes::job_logs))
        .route(
            "/api/source",
            get(routes::source_page)
                .put(routes::source_write)
                .layer(DefaultBodyLimit::max(SOURCE_BODY_BYTES)),
        )
        .route("/api/files/{*path}", get(files::serve).head(files::serve))
        .route("/api/events", get(events::stream))
        .fallback(workbench::page)
        .with_state(state.clone());
    // Layered around the whole router rather than each route, so every
    // request passes the guards and a 405 already carries its `Allow`.
    Router::new()
        .fallback_service(routes)
        .layer(DefaultBodyLimit::max(REQUEST_BODY_BYTES))
        .layer(middleware::map_response(error::method_not_allowed))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::guard_api,
        ))
        .layer(middleware::from_fn_with_state(state, auth::check_host))
        .layer(middleware::from_fn(common_headers))
        .layer(TraceLayer::new_for_http())
}

async fn common_headers(request: Request, next: Next) -> Response {
    let api = auth::is_api(request.uri().path());
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    if api {
        headers
            .entry(header::CACHE_CONTROL)
            .or_insert(HeaderValue::from_static("no-store"));
    }
    response
}
