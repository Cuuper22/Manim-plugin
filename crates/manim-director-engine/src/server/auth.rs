//! Who may talk to the server (HTTP §2): the per-process session token, the
//! `?token=` bootstrap that turns it into a cookie, and the Host, Origin and
//! Content-Type checks that keep websites and DNS rebinding out.

use super::{error::ApiError, state::AppState};
use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri},
    middleware::Next,
    response::{IntoResponse, Response},
};
use percent_encoding::percent_decode_str;
use std::{hint::black_box, io};

const TOKEN_BYTES: usize = 32;
const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const LOOPBACK_NAMES: [&str; 3] = ["localhost", "127.0.0.1", "[::1]"];

/// The one credential of a server process; never persisted.
pub struct Session {
    token: String,
    cookie: String,
}

impl Session {
    /// 32 bytes from the OS CSPRNG, base64url without padding.
    pub fn new(port: u16) -> io::Result<Self> {
        let mut bytes = [0_u8; TOKEN_BYTES];
        getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
        Ok(Self {
            token: base64url(&bytes),
            cookie: format!("mdsess_{port}"),
        })
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// Bearer or the session cookie; the port in the cookie name keeps two
    /// engines on one host from clobbering each other's cookie.
    pub fn accepts(&self, headers: &HeaderMap) -> bool {
        let bearer = headers
            .get_all(header::AUTHORIZATION)
            .iter()
            .filter_map(|value| value.to_str().ok()?.strip_prefix("Bearer "))
            .any(|token| self.is_token(token.trim()));
        bearer || cookies(headers).any(|(name, value)| name == self.cookie && self.is_token(value))
    }

    fn is_token(&self, candidate: &str) -> bool {
        constant_time_eq(self.token.as_bytes(), candidate.as_bytes())
    }

    fn set_cookie(&self) -> HeaderValue {
        HeaderValue::from_str(&format!(
            "{}={}; Path=/; HttpOnly; SameSite=Strict",
            self.cookie, self.token
        ))
        .expect("the cookie is ASCII")
    }

    /// `GET /<path>?token=…`: a 303 to the same path without the token,
    /// setting the cookie only for the right token (HTTP §2.4).
    pub fn bootstrap(&self, uri: &Uri) -> Option<Response> {
        let query = uri.query()?;
        let mut token = None;
        let mut kept = Vec::new();
        for pair in query.split('&') {
            match pair.split_once('=').unwrap_or((pair, "")) {
                ("token", value) => token = Some(percent_decode_str(value).decode_utf8_lossy()),
                _ => kept.push(pair),
            }
        }
        let token = token?;
        // A leading `//` or `/\` would make the relative Location point off-site.
        let mut location = format!("/{}", uri.path().trim_start_matches(['/', '\\']));
        if !kept.is_empty() {
            location = format!("{location}?{}", kept.join("&"));
        }
        let mut response = (
            StatusCode::SEE_OTHER,
            [
                (header::LOCATION, location),
                (header::CACHE_CONTROL, "no-store".to_owned()),
            ],
        )
            .into_response();
        if self.is_token(&token) {
            response
                .headers_mut()
                .insert(header::SET_COOKIE, self.set_cookie());
        }
        Some(response)
    }
}

fn cookies(headers: &HeaderMap) -> impl Iterator<Item = (&str, &str)> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
}

fn constant_time_eq(expected: &[u8], candidate: &[u8]) -> bool {
    if expected.len() != candidate.len() {
        return false;
    }
    let difference = expected
        .iter()
        .zip(candidate)
        .fold(0_u8, |acc, (a, b)| black_box(acc | (a ^ b)));
    difference == 0
}

fn base64url(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let block = chunk.iter().enumerate().fold(0_u32, |block, (i, byte)| {
            block | (u32::from(*byte) << (16 - 8 * i))
        });
        for i in 0..=chunk.len() {
            encoded.push(BASE64URL[((block >> (18 - 6 * i)) & 0x3f) as usize] as char);
        }
    }
    encoded
}

/// HTTP §2.5: exactly `localhost`, `127.0.0.1` or `[::1]`, with the bound
/// port (omitted only for port 80). No userinfo, no trailing dot.
pub fn host_allowed(host: &str, port: u16) -> bool {
    let host = host.to_ascii_lowercase();
    let (name, written_port) = match host.strip_prefix('[') {
        Some(rest) => match rest.split_once(']') {
            Some((inside, "")) => (format!("[{inside}]"), None),
            Some((inside, after)) => match after.strip_prefix(':') {
                Some(port) => (format!("[{inside}]"), Some(port.to_owned())),
                None => return false,
            },
            None => return false,
        },
        None => match host.split_once(':') {
            Some((name, port)) => (name.to_owned(), Some(port.to_owned())),
            None => (host.clone(), None),
        },
    };
    let port_matches = match written_port {
        Some(written) => written == port.to_string(),
        None => port == 80,
    };
    LOOPBACK_NAMES.contains(&name.as_str()) && port_matches
}

/// Every request: refuse Host headers that do not name this loopback server
/// (DNS rebinding), unless remote access was asked for.
pub async fn check_host(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if !state.allow_remote {
        let mut hosts = request.headers().get_all(header::HOST).iter();
        let host = hosts.next().map(|value| value.to_str().unwrap_or_default());
        let allowed =
            hosts.next().is_none() && host.is_some_and(|host| host_allowed(host, state.port));
        if !allowed {
            return ApiError::forbidden_host(host).into_response();
        }
    }
    next.run(request).await
}

pub fn is_api(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/")
}

/// `/api`: a credential always; for writes also a same-origin `Origin` (when
/// sent) and a JSON content type, which a cross-site form cannot send.
pub async fn guard_api(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if !is_api(request.uri().path()) {
        return next.run(request).await;
    }
    let headers = request.headers();
    if !state.session.accepts(headers) {
        return ApiError::unauthorized().into_response();
    }
    if matches!(*request.method(), Method::POST | Method::PUT) {
        if let Some(origin) = headers.get(header::ORIGIN) {
            let origin = origin.to_str().unwrap_or_default().to_ascii_lowercase();
            let expected = headers
                .get(header::HOST)
                .and_then(|host| host.to_str().ok())
                .map(|host| format!("http://{}", host.to_ascii_lowercase()));
            if expected.as_deref() != Some(origin.as_str()) {
                return ApiError::forbidden_origin(&origin).into_response();
            }
        }
        let json = headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("application/json"));
        if !json {
            return ApiError::unsupported_media_type().into_response();
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_43_base64url_characters_and_differ() {
        let first = Session::new(4177).unwrap();
        let second = Session::new(4177).unwrap();
        assert_eq!(first.token().len(), 43);
        assert!(first.token().bytes().all(|byte| BASE64URL.contains(&byte)));
        assert_ne!(first.token(), second.token());
        assert_eq!(base64url(b"\xfb\xff"), "-_8");
        assert_eq!(
            base64url(b"any carnal pleasure."),
            "YW55IGNhcm5hbCBwbGVhc3VyZS4"
        );
    }

    #[test]
    fn hosts_must_be_loopback_names_with_the_bound_port() {
        for allowed in [
            "localhost:4177",
            "127.0.0.1:4177",
            "[::1]:4177",
            "LOCALHOST:4177",
        ] {
            assert!(host_allowed(allowed, 4177), "{allowed}");
        }
        for refused in [
            "evil.com:4177",
            "localhost:9999",
            "localhost.:4177",
            "localhost",
            "user@localhost:4177",
            "127.0.0.2:4177",
            "[::1]",
            "[::1]x:4177",
            "localhost:04177",
            "",
        ] {
            assert!(!host_allowed(refused, 4177), "{refused}");
        }
        assert!(host_allowed("localhost", 80));
    }

    #[test]
    fn bootstrap_strips_the_token_and_never_redirects_off_site() {
        let session = Session::new(4177).unwrap();
        let uri: Uri = format!("//evil.com/x?a=1&token={}&b=2", session.token())
            .parse()
            .unwrap();
        let response = session.bootstrap(&uri).unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[header::LOCATION], "/evil.com/x?a=1&b=2");
        let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
        assert_eq!(
            cookie,
            format!(
                "mdsess_4177={}; Path=/; HttpOnly; SameSite=Strict",
                session.token()
            )
        );
        let wrong = session.bootstrap(&"/?token=nope".parse().unwrap()).unwrap();
        assert_eq!(wrong.headers()[header::LOCATION], "/");
        assert!(wrong.headers().get(header::SET_COOKIE).is_none());
        assert!(session.bootstrap(&"/?a=1".parse().unwrap()).is_none());
    }
}
