//! The workbench page (HTTP §6.4): embedded at build time or served from
//! `--workbench-dir`, with SPA fallback to `index.html` and the token
//! bootstrap. Assets hold no data, so they need no credential.

use super::{auth::is_api, error::ApiError, state::AppState};
use axum::{
    body::Body,
    extract::State,
    http::{header, Method, Uri},
    response::{IntoResponse, Response},
};
use manim_director_core::path_rule_violation;
use percent_encoding::percent_decode_str;
use std::{borrow::Cow, path::PathBuf};

include!(concat!(env!("OUT_DIR"), "/embedded_workbench.rs"));

const INDEX: &str = "index.html";
const PAGE_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
    img-src 'self' data: blob:; media-src 'self' blob:; connect-src 'self'; frame-ancestors 'none'; \
    base-uri 'none'; form-action 'none'";

pub enum Workbench {
    Embedded,
    Directory(PathBuf),
}

impl Workbench {
    /// The asset at `path`, else `index.html`; with the name actually served.
    async fn asset(&self, path: &str) -> Option<(Cow<'static, [u8]>, String)> {
        match self {
            Self::Embedded => embedded_asset(path)
                .map(|bytes| (Cow::Borrowed(bytes), path.to_owned()))
                .or_else(|| Some((Cow::Borrowed(embedded_asset(INDEX)?), INDEX.to_owned()))),
            Self::Directory(directory) => {
                let file = safe_relative(path).map(|relative| directory.join(relative));
                if let Some(file) = file.filter(|file| file.is_file()) {
                    if let Ok(bytes) = tokio::fs::read(&file).await {
                        return Some((Cow::Owned(bytes), path.to_owned()));
                    }
                }
                let index = tokio::fs::read(directory.join(INDEX)).await.ok()?;
                Some((Cow::Owned(index), INDEX.to_owned()))
            }
        }
    }
}

/// A request path that obeys the project path rule, so it cannot name a
/// drive, a parent or a hidden file.
fn safe_relative(path: &str) -> Option<String> {
    let decoded = percent_decode_str(path).decode_utf8().ok()?;
    path_rule_violation(&decoded, false)
        .is_none()
        .then(|| decoded.into_owned())
}

/// `text/*` and scripts declare UTF-8; everything else is mime_guess's guess.
fn asset_type(path: &str) -> String {
    let guess = mime_guess::from_path(path).first_or_octet_stream();
    let textual = guess.type_() == mime_guess::mime::TEXT
        || matches!(
            guess.essence_str(),
            "application/javascript" | "application/json"
        );
    match textual {
        true => format!("{}; charset=utf-8", guess.essence_str()),
        false => guess.essence_str().to_owned(),
    }
}

/// Every request no route matched.
pub async fn page(State(state): State<AppState>, method: Method, uri: Uri) -> Response {
    if is_api(uri.path()) {
        return ApiError::route_not_found(uri.path()).into_response();
    }
    if !matches!(method, Method::GET | Method::HEAD) {
        return ApiError::method_not_allowed(vec!["GET".into(), "HEAD".into()]).into_response();
    }
    if let Some(redirect) = state.session.bootstrap(&uri) {
        return redirect;
    }
    let requested = match uri.path().trim_start_matches('/') {
        "" => INDEX,
        path => path,
    };
    let Some((bytes, served)) = state.workbench.asset(requested).await else {
        return ApiError::route_not_found(uri.path()).into_response();
    };
    let mut response = Response::builder().header(header::CONTENT_TYPE, asset_type(&served));
    response = match served == INDEX {
        true => response
            .header(header::CACHE_CONTROL, "no-cache")
            .header(header::CONTENT_SECURITY_POLICY, PAGE_CSP),
        false => response.header(header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
    };
    let body = match bytes {
        Cow::Borrowed(bytes) => Body::from(bytes),
        Cow::Owned(bytes) => Body::from(bytes),
    };
    response.body(body).expect("valid headers")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_types_declare_utf8_for_text() {
        assert_eq!(asset_type("index.html"), "text/html; charset=utf-8");
        assert_eq!(
            asset_type("assets/app-1a2b.js"),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(asset_type("assets/app.css"), "text/css; charset=utf-8");
        assert_eq!(asset_type("icon.svg"), "image/svg+xml");
        assert_eq!(asset_type("font.woff2"), "font/woff2");
        assert_eq!(asset_type("blob"), "application/octet-stream");
    }

    #[test]
    fn directory_assets_never_leave_the_directory() {
        assert_eq!(
            safe_relative("assets/app.js").as_deref(),
            Some("assets/app.js")
        );
        for refused in [
            "../secret",
            "%2e%2e/secret",
            ".env",
            "a//b",
            "a\\b",
            "C:/Windows/win.ini",
            "C:x",
            "c%3A/x",
            "index.html::$DATA",
        ] {
            assert_eq!(safe_relative(refused), None, "{refused}");
        }
    }

    #[test]
    fn the_embedded_bundle_always_has_an_index() {
        assert!(embedded_asset(INDEX).is_some());
    }
}
