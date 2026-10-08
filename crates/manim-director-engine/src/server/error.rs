//! The error envelope (HTTP §3) and the mapping of framework rejections
//! into it, so clients only ever branch on `error.code`.

use axum::{
    extract::{
        rejection::{BytesRejection, FailedToBufferBody},
        FromRequestParts, Path as AxumPath, Query,
    },
    http::{header, request::Parts, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use manim_director_core::{parse_value, EngineError, ErrorBody, Resource};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use uuid::Uuid;

pub type ApiResult<T> = Result<T, ApiError>;

/// Boxed: handlers return it on every error path, so it stays one pointer.
#[derive(Debug)]
pub struct ApiError(Box<Rejection>);

#[derive(Debug)]
struct Rejection {
    status: StatusCode,
    body: ErrorBody,
    header: Option<(HeaderName, HeaderValue)>,
}

impl ApiError {
    fn new(
        status: StatusCode,
        code: &str,
        message: impl Into<String>,
        data: Option<Value>,
    ) -> Self {
        Self(Box::new(Rejection {
            status,
            body: ErrorBody::new(code, message, data),
            header: None,
        }))
    }

    fn with_header(mut self, name: HeaderName, value: impl Into<HeaderValue>) -> Self {
        self.0.header = Some((name, value.into()));
        self
    }

    #[cfg(test)]
    pub fn code(&self) -> &str {
        &self.0.body.code
    }

    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Open the workbench link printed by `manim-director open`, or send the session token as a Bearer credential.",
            None,
        )
        .with_header(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static(r#"Bearer realm="manim-director""#),
        )
    }

    pub fn forbidden_host(host: Option<&str>) -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "forbidden_host",
            "The Host header must name this loopback server; use --allow-remote to serve other hosts.",
            Some(json!({ "host": host })),
        )
    }

    pub fn forbidden_origin(origin: &str) -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "forbidden_origin",
            "Requests from other origins are not accepted.",
            Some(json!({ "origin": origin })),
        )
    }

    pub fn unsupported_media_type() -> Self {
        Self::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            "Send a JSON body with Content-Type: application/json.",
            None,
        )
    }

    pub fn method_not_allowed(allowed: Vec<String>) -> Self {
        Self::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            format!("This route accepts {}.", allowed.join(", ")),
            Some(json!({ "allowed": allowed })),
        )
    }

    pub fn route_not_found(path: &str) -> Self {
        EngineError::NotFound {
            resource: Resource::Route,
            key: path.to_owned(),
        }
        .into()
    }

    pub fn artifact_changed(path: &str, requested: &str, current: &str) -> Self {
        Self::new(
            StatusCode::GONE,
            "artifact_changed",
            format!("{path} changed since this link was made; reload the workspace."),
            Some(json!({
                "path": path,
                "requested_version": requested,
                "current_version": current,
            })),
        )
    }

    pub fn range_not_satisfiable(size: u64) -> Self {
        Self::new(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "range_not_satisfiable",
            format!("The requested range is outside the file's {size} bytes."),
            Some(json!({ "size": size })),
        )
        .with_header(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes */{size}")).expect("ASCII"),
        )
    }

    pub fn too_many_streams(limit: usize) -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_streams",
            format!("At most {limit} event streams may be open at once."),
            Some(json!({ "limit": limit })),
        )
    }

    pub fn body_rejection(rejection: BytesRejection, limit_bytes: usize) -> Self {
        match rejection {
            BytesRejection::FailedToBufferBody(FailedToBufferBody::LengthLimitError(_)) => {
                EngineError::RequestTooLarge {
                    limit_bytes: limit_bytes as u64,
                    actual_bytes: None,
                }
                .into()
            }
            other => EngineError::invalid_request(other.body_text()).into(),
        }
    }
}

impl From<EngineError> for ApiError {
    fn from(error: EngineError) -> Self {
        let status =
            StatusCode::from_u16(error.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = match &error {
            EngineError::Internal(detail) => {
                tracing::error!(%detail, "internal error");
                ErrorBody::internal("Internal error; see the engine log.")
            }
            _ => error.body(),
        };
        let header = matches!(error, EngineError::QueueFull { .. })
            .then(|| (header::RETRY_AFTER, HeaderValue::from_static("2")));
        Self(Box::new(Rejection {
            status,
            body,
            header,
        }))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let Rejection {
            status,
            body,
            header,
        } = *self.0;
        let mut response = (status, Json(json!({ "error": body }))).into_response();
        if let Some((name, value)) = header {
            response.headers_mut().insert(name, value);
        }
        response
    }
}

/// Parses a JSON body strictly; the error names the offending field.
pub fn parse_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, EngineError> {
    parse_value(json_value(bytes)?)
}

/// Any well-formed JSON document.
pub fn json_value(bytes: &[u8]) -> Result<Value, EngineError> {
    serde_json::from_slice(bytes)
        .map_err(|error| EngineError::invalid_request(format!("malformed JSON: {error}")))
}

/// Turns the router's bare 405 into the envelope, keeping its `Allow` header.
pub async fn method_not_allowed(response: Response) -> Response {
    if response.status() != StatusCode::METHOD_NOT_ALLOWED {
        return response;
    }
    let allow = response.headers().get(header::ALLOW).cloned();
    let allowed = allow
        .as_ref()
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(',')
                .map(|method| method.trim().to_owned())
                .collect()
        })
        .unwrap_or_default();
    let mut mapped = ApiError::method_not_allowed(allowed).into_response();
    if let Some(allow) = allow {
        mapped.headers_mut().insert(header::ALLOW, allow);
    }
    mapped
}

/// `Query<T>` whose rejection is the envelope's `invalid_params`.
pub struct ApiQuery<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequestParts<S> for ApiQuery<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        match Query::<T>::from_request_parts(parts, state).await {
            Ok(Query(value)) => Ok(Self(value)),
            Err(rejection) => Err(EngineError::invalid_request(rejection.body_text()).into()),
        }
    }
}

/// The `{id}` of a job route; anything but a UUID is `invalid_params`.
pub struct JobId(pub Uuid);

impl<S: Send + Sync> FromRequestParts<S> for JobId {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        let id = AxumPath::<String>::from_request_parts(parts, state)
            .await
            .ok()
            .and_then(|AxumPath(id)| id.parse().ok());
        id.map(Self)
            .ok_or_else(|| EngineError::invalid("id", "not a job id").into())
    }
}
