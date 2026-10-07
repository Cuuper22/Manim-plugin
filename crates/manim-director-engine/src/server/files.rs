//! `GET|HEAD /api/files/{*path}` (HTTP §7): confined, versioned file
//! streaming with single byte ranges for video seeking.

use super::{
    error::{ApiError, ApiQuery, ApiResult},
    state::AppState,
};
use crate::workspace::{content_type, file_version};
use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri},
    response::Response,
};
use chrono::{DateTime, Utc};
use manim_director_core::{files, path_rule_violation, EngineError, Resource};
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::Deserialize;
use std::{
    fs,
    io::SeekFrom,
    path::{Path, PathBuf},
};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

const MAX_SERVED_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const ROUTE_PREFIX: &str = "/api/files/";
/// A same-origin SVG opened directly must not run script with the cookie.
const FILE_CSP: &str =
    "sandbox; default-src 'none'; img-src 'self' data:; media-src 'self'; style-src 'unsafe-inline'";
/// RFC 8187 `attr-char`s pass through; everything else is encoded.
const ATTR_CHAR: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'!')
    .remove(b'#')
    .remove(b'$')
    .remove(b'&')
    .remove(b'+')
    .remove(b'-')
    .remove(b'.')
    .remove(b'^')
    .remove(b'_')
    .remove(b'`')
    .remove(b'|')
    .remove(b'~');

#[derive(Debug, Deserialize)]
pub struct FileQuery {
    v: Option<String>,
    download: Option<String>,
}

struct Served {
    path: String,
    file: PathBuf,
    metadata: fs::Metadata,
}

pub async fn serve(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    ApiQuery(query): ApiQuery<FileQuery>,
) -> ApiResult<Response> {
    let encoded = uri.path().strip_prefix(ROUTE_PREFIX).unwrap_or_default();
    let path = decode(encoded)?;
    let reader = state.clone();
    let served = tokio::task::spawn_blocking(move || {
        let media_dir = reader.load_spec().spec.project.media_dir.clone();
        locate(reader.root(), &path, &media_dir)
    })
    .await
    .map_err(EngineError::internal)??;
    let version = file_version(&served.metadata);
    if let Some(requested) = query.v.as_deref().filter(|requested| *requested != version) {
        return Err(ApiError::artifact_changed(
            &served.path,
            requested,
            &version,
        ));
    }
    let size = served.metadata.len();
    let etag = format!("\"{version}\"");
    let mut response = Response::builder().header(header::ETAG, &etag).header(
        header::CACHE_CONTROL,
        match query.v.is_some() {
            true => "private, max-age=31536000, immutable",
            false => "no-cache",
        },
    );
    if let Some(modified) = served.metadata.modified().ok().map(DateTime::<Utc>::from) {
        response = response.header(
            header::LAST_MODIFIED,
            modified.format("%a, %d %b %Y %H:%M:%S GMT").to_string(),
        );
    }
    if headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|tag| tag.trim() == etag || tag.trim() == "*")
    {
        return Ok(response
            .status(StatusCode::NOT_MODIFIED)
            .body(Body::empty())
            .expect("valid headers"));
    }
    let (status, start, length) = match requested_range(&headers, size, &etag)? {
        Some((start, end)) => {
            response =
                response.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"));
            (StatusCode::PARTIAL_CONTENT, start, end - start + 1)
        }
        None => (StatusCode::OK, 0, size),
    };
    let attachment = query.download.as_deref() == Some("1")
        || files::extension(&served.path).as_deref() == Some("pdf");
    let response = response
        .status(status)
        .header(header::CONTENT_TYPE, content_type(&served.path))
        .header(header::CONTENT_LENGTH, length)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(
            header::CONTENT_DISPOSITION,
            disposition(&served.path, attachment),
        )
        .header(header::CONTENT_SECURITY_POLICY, FILE_CSP)
        .header("cross-origin-resource-policy", "same-origin");
    let body = match method {
        Method::HEAD => Body::empty(),
        _ => {
            let mut file = tokio::fs::File::open(&served.file)
                .await
                .map_err(EngineError::internal)?;
            file.seek(SeekFrom::Start(start))
                .await
                .map_err(EngineError::internal)?;
            Body::from_stream(ReaderStream::new(file.take(length)))
        }
    };
    Ok(response.body(body).expect("valid headers"))
}

/// Percent-decodes each segment on its own, so an encoded `/` can never
/// smuggle in an extra segment.
fn decode(encoded: &str) -> Result<String, EngineError> {
    let segments = encoded
        .split('/')
        .map(|segment| {
            percent_decode_str(segment)
                .decode_utf8()
                .ok()
                .filter(|segment| !segment.contains(['/', '\\', '\0']))
        })
        .collect::<Option<Vec<_>>>();
    segments
        .map(|segments| segments.join("/"))
        .ok_or_else(|| EngineError::InvalidPath {
            path: encoded.to_owned(),
            reason: "absolute",
        })
}

/// The path rules of HTTP §7.1, checked on the request path and again on
/// what it canonicalizes to. Blocking.
fn locate(root: &Path, path: &str, media_dir: &str) -> Result<Served, EngineError> {
    let invalid = |reason| EngineError::InvalidPath {
        path: path.to_owned(),
        reason,
    };
    check_served(path, media_dir).map_err(invalid)?;
    let extension = files::extension(path).unwrap_or_default();
    if !files::DOWNLOADABLE.contains(&extension.as_str()) {
        return Err(EngineError::UnsupportedFileType {
            path: path.to_owned(),
            extension,
        });
    }
    let missing = || EngineError::NotFound {
        resource: Resource::File,
        key: path.to_owned(),
    };
    let file = root.join(path).canonicalize().map_err(|_| missing())?;
    let resolved = file
        .strip_prefix(root)
        .map_err(|_| invalid("outside_project"))?
        .to_string_lossy()
        .replace('\\', "/");
    check_served(&resolved, media_dir).map_err(invalid)?;
    if !files::has_extension(&resolved, files::DOWNLOADABLE) {
        return Err(invalid("denied"));
    }
    let metadata = file.metadata().map_err(|_| missing())?;
    if !metadata.is_file() {
        return Err(missing());
    }
    if metadata.len() > MAX_SERVED_BYTES {
        return Err(EngineError::FileTooLarge {
            path: path.to_owned(),
            bytes: metadata.len(),
            limit_bytes: MAX_SERVED_BYTES,
        });
    }
    Ok(Served {
        path: path.to_owned(),
        file,
        metadata,
    })
}

/// Lexical rules plus the denied media cache; job artifacts are the only
/// hidden files served.
fn check_served(path: &str, media_dir: &str) -> Result<(), &'static str> {
    if let Some(reason) = path_rule_violation(path, true) {
        return Err(reason);
    }
    let media: Vec<&str> = media_dir
        .split('/')
        .filter(|part| !matches!(*part, "" | "."))
        .collect();
    let under_media =
        !media.is_empty() && path.split('/').take(media.len()).eq(media.iter().copied());
    match under_media {
        true => Err("denied"),
        false => Ok(()),
    }
}

/// One `bytes=` range (HTTP §7.3) as an inclusive `(start, end)`; `None`
/// serves the whole file.
fn requested_range(
    headers: &HeaderMap,
    size: u64,
    etag: &str,
) -> Result<Option<(u64, u64)>, ApiError> {
    let mut values = headers.get_all(header::RANGE).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    let unsatisfiable = || ApiError::range_not_satisfiable(size);
    if values.next().is_some() {
        return Err(unsatisfiable());
    }
    if let Some(validator) = headers.get(header::IF_RANGE) {
        if validator.as_bytes() != etag.as_bytes() {
            return Ok(None);
        }
    }
    let value = value.to_str().map_err(|_| unsatisfiable())?;
    let Some(spec) = value.strip_prefix("bytes=") else {
        return Ok(None);
    };
    let number = |digits: &str| -> Option<u64> {
        let plain = (1..=19).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit());
        plain.then(|| digits.parse().ok()).flatten()
    };
    let (start, end) = match spec.split_once('-') {
        Some(("", suffix)) => {
            let length = number(suffix).ok_or_else(unsatisfiable)?;
            if length == 0 || size == 0 {
                return Err(unsatisfiable());
            }
            (size.saturating_sub(length), size - 1)
        }
        Some((first, last)) => {
            let start = number(first).ok_or_else(unsatisfiable)?;
            let end = match last {
                "" => None,
                last => Some(number(last).ok_or_else(unsatisfiable)?),
            };
            if start >= size || end.is_some_and(|end| start > end) {
                return Err(unsatisfiable());
            }
            (start, end.map_or(size - 1, |end| end.min(size - 1)))
        }
        None => return Err(unsatisfiable()),
    };
    Ok(Some((start, end)))
}

fn disposition(path: &str, attachment: bool) -> HeaderValue {
    if !attachment {
        return HeaderValue::from_static("inline");
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let fallback: String = name
        .chars()
        .map(|c| match c {
            ' '..='~' if c != '"' && c != '\\' => c,
            _ => '_',
        })
        .collect();
    let encoded = utf8_percent_encode(name, ATTR_CHAR);
    HeaderValue::from_str(&format!(
        "attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}"
    ))
    .expect("the disposition is ASCII")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(value: &str, size: u64) -> Result<Option<(u64, u64)>, String> {
        let mut headers = HeaderMap::new();
        headers.insert(header::RANGE, HeaderValue::from_str(value).unwrap());
        requested_range(&headers, size, "\"v\"").map_err(|error| error.code().to_owned())
    }

    #[test]
    fn byte_ranges_follow_the_contract_grammar() {
        assert_eq!(range("bytes=0-99", 1000), Ok(Some((0, 99))));
        assert_eq!(range("bytes=900-", 1000), Ok(Some((900, 999))));
        assert_eq!(range("bytes=990-5000", 1000), Ok(Some((990, 999))));
        assert_eq!(range("bytes=-100", 1000), Ok(Some((900, 999))));
        assert_eq!(range("bytes=-1005", 1000), Ok(Some((0, 999))));
        assert_eq!(range("items=0-1", 1000), Ok(None));
        for refused in [
            "bytes=1000-",
            "bytes=5-2",
            "bytes=-0",
            "bytes=0-1,4-5",
            "bytes= 0-1",
            "bytes=0-1 ",
            "bytes=",
            "bytes=-",
            "bytes=abc",
            "bytes=99999999999999999999-",
        ] {
            assert_eq!(
                range(refused, 1000),
                Err("range_not_satisfiable".into()),
                "{refused}"
            );
        }
        assert_eq!(range("bytes=0-", 0), Err("range_not_satisfiable".into()));
    }

    #[test]
    fn if_range_applies_the_range_only_for_the_current_etag() {
        let mut headers = HeaderMap::new();
        headers.insert(header::RANGE, HeaderValue::from_static("bytes=0-1"));
        headers.insert(header::IF_RANGE, HeaderValue::from_static("\"old\""));
        assert_eq!(requested_range(&headers, 10, "\"v\"").unwrap(), None);
        headers.insert(header::IF_RANGE, HeaderValue::from_static("\"v\""));
        assert_eq!(
            requested_range(&headers, 10, "\"v\"").unwrap(),
            Some((0, 1))
        );
    }

    #[test]
    fn served_paths_exclude_engine_state_and_the_media_cache() {
        let allowed = ".manim-director/artifacts/7f/Recurrence.mp4";
        assert_eq!(check_served(allowed, ".manim-director/media"), Ok(()));
        assert_eq!(check_served("output/final.mp4", "media"), Ok(()));
        for (path, reason) in [
            (".manim-director/state.db", "hidden"),
            (".manim-director/media/x.mp4", "hidden"),
            ("media/videos/x.mp4", "denied"),
            ("../x.mp4", "traversal"),
            ("/etc/x.mp4", "absolute"),
            (".github/x.yml", "hidden"),
        ] {
            assert_eq!(check_served(path, "media"), Err(reason), "{path}");
        }
        assert_eq!(decode("a%2Fb.mp4").unwrap_err().code(), "invalid_path");
        assert_eq!(decode("Re%20cur.mp4").unwrap(), "Re cur.mp4");
    }

    #[test]
    fn attachments_name_the_file_in_both_forms() {
        assert_eq!(disposition("output/a.mp4", false), "inline");
        assert_eq!(
            disposition("output/résumé \"1\".pdf", true),
            "attachment; filename=\"r_sum_ _1_.pdf\"; filename*=UTF-8''r%C3%A9sum%C3%A9%20%221%22.pdf"
        );
    }
}
