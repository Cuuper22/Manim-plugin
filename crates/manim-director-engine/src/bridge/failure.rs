//! The engine-side failure codes of a bridge conversation (OPS §1.6).

use manim_director_core::{ErrorBody, PROTOCOL_VERSION};
use serde_json::json;
use std::{path::Path, process::ExitStatus};

pub(super) fn runtime_unavailable(python: &Path, detail: &str, stderr_tail: &str) -> ErrorBody {
    ErrorBody::new(
        "runtime_unavailable",
        format!(
            "The Python runtime at {} is unavailable: {detail}.",
            python.display()
        ),
        Some(json!({"python": python, "stderr_tail": stderr_tail})),
    )
}

pub(super) fn runtime_protocol(detail: &str, stderr_tail: &str) -> ErrorBody {
    ErrorBody::new(
        "runtime_protocol",
        format!("The runtime broke the bridge protocol: {detail}."),
        Some(json!({"detail": detail, "stderr_tail": stderr_tail})),
    )
}

pub(super) fn protocol_mismatch(got: u32, stderr_tail: &str) -> ErrorBody {
    ErrorBody::new(
        "runtime_protocol",
        format!("The runtime speaks bridge protocol {got}; this engine needs {PROTOCOL_VERSION}."),
        Some(json!({
            "detail": "protocol mismatch",
            "expected": PROTOCOL_VERSION,
            "got": got,
            "hint": format!("reinstall the runtime matching engine {}", env!("CARGO_PKG_VERSION")),
            "stderr_tail": stderr_tail,
        })),
    )
}

pub(super) fn runtime_crashed(status: Option<ExitStatus>, stderr_tail: &str) -> ErrorBody {
    let exit_code = status.and_then(|status| status.code());
    #[cfg(unix)]
    let signal = status.and_then(|status| std::os::unix::process::ExitStatusExt::signal(&status));
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    ErrorBody::new(
        "runtime_crashed",
        format!(
            "The runtime exited without a result ({}).",
            describe_exit(status)
        ),
        Some(json!({"exit_code": exit_code, "signal": signal, "stderr_tail": stderr_tail})),
    )
}

pub(super) fn describe_exit(status: Option<ExitStatus>) -> String {
    status.map_or_else(|| "exit status unknown".into(), |status| status.to_string())
}

/// The `stderr_tail` an error carries, for logs.
pub(super) fn stderr_tail(error: &ErrorBody) -> &str {
    error
        .data
        .as_ref()
        .and_then(|data| data.get("stderr_tail"))
        .and_then(|tail| tail.as_str())
        .unwrap_or_default()
}

pub(super) fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    format!("{}…", &value[..value.floor_char_boundary(max)])
}
