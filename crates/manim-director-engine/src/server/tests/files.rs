//! HTTP §7 and §14 items 30–39: confined, versioned streaming with ranges.

use super::*;
use crate::workspace::file_version;

const SIZE: usize = 1000;

fn bytes() -> Vec<u8> {
    (0..SIZE).map(|n| (n % 251) as u8).collect()
}

fn put(harness: &Harness, path: &str, content: &[u8]) -> String {
    let file = harness.root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, content).unwrap();
    file_version(&file.metadata().unwrap())
}

async fn ranged(harness: &Harness, method: Method, path: &str, ranges: &[&str]) -> Reply {
    let mut request = harness.request(method, path);
    for range in ranges {
        request = request.header(header::RANGE, *range);
    }
    harness.send(request.body(Body::empty()).unwrap()).await
}

#[tokio::test]
async fn whole_files_and_single_ranges_stream_with_contract_headers() {
    let harness = Harness::new("").await;
    let version = put(&harness, "output/clip.mp4", &bytes());
    let url = format!("/api/files/output/clip.mp4?v={version}");
    let whole = harness.get(&url).await;
    assert_eq!(whole.status, StatusCode::OK);
    assert_eq!(whole.body.as_ref(), bytes().as_slice());
    assert_eq!(whole.header(header::ACCEPT_RANGES), "bytes");
    assert_eq!(whole.header(header::CONTENT_TYPE), "video/mp4");
    assert_eq!(whole.header(header::CONTENT_LENGTH), "1000");
    assert_eq!(whole.header(header::ETAG), format!("\"{version}\""));
    assert_eq!(
        whole.header(header::CACHE_CONTROL),
        "private, max-age=31536000, immutable"
    );
    assert_eq!(whole.header(header::CONTENT_DISPOSITION), "inline");
    assert_eq!(whole.header("cross-origin-resource-policy"), "same-origin");
    assert!(whole
        .header(header::CONTENT_SECURITY_POLICY)
        .starts_with("sandbox;"));
    assert!(whole.header(header::LAST_MODIFIED).ends_with(" GMT"));
    let unversioned = harness.get("/api/files/output/clip.mp4").await;
    assert_eq!(unversioned.header(header::CACHE_CONTROL), "no-cache");

    let head = ranged(&harness, Method::GET, &url, &["bytes=0-99"]).await;
    assert_eq!(head.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(head.header(header::CONTENT_RANGE), "bytes 0-99/1000");
    assert_eq!(head.header(header::CONTENT_LENGTH), "100");
    assert_eq!(head.body.as_ref(), &bytes()[..100]);
    let tail = ranged(&harness, Method::GET, &url, &["bytes=-100"]).await;
    assert_eq!(tail.header(header::CONTENT_RANGE), "bytes 900-999/1000");
    assert_eq!(tail.body.as_ref(), &bytes()[900..]);
    let oversized = ranged(&harness, Method::GET, &url, &["bytes=-1005"]).await;
    assert_eq!(oversized.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(oversized.header(header::CONTENT_RANGE), "bytes 0-999/1000");
    let open_ended = ranged(&harness, Method::GET, &url, &["bytes=990-"]).await;
    assert_eq!(open_ended.body.as_ref(), &bytes()[990..]);

    for invalid in [
        &["bytes=1000-"][..],
        &["bytes=5-2"],
        &["bytes=-0"],
        &["bytes=0-1,4-5"],
        &["bytes= 0-1"],
        &["bytes=0-1", "bytes=2-3"],
    ] {
        let reply = ranged(&harness, Method::GET, &url, invalid).await;
        assert_eq!(
            reply.error(StatusCode::RANGE_NOT_SATISFIABLE),
            "range_not_satisfiable"
        );
        assert_eq!(
            reply.header(header::CONTENT_RANGE),
            "bytes */1000",
            "{invalid:?}"
        );
        assert_eq!(reply.json()["error"]["data"]["size"], 1000);
    }

    let head = ranged(&harness, Method::HEAD, &url, &[]).await;
    assert_eq!(head.status, StatusCode::OK);
    assert!(head.body.is_empty());
    assert_eq!(head.header(header::CONTENT_LENGTH), "1000");
    let head_range = ranged(&harness, Method::HEAD, &url, &["bytes=10-19"]).await;
    assert_eq!(head_range.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(head_range.header(header::CONTENT_LENGTH), "10");
    assert!(head_range.body.is_empty());

    let cached = harness
        .request(Method::GET, &url)
        .header(header::IF_NONE_MATCH, format!("\"{version}\""))
        .body(Body::empty())
        .unwrap();
    assert_eq!(harness.send(cached).await.status, StatusCode::NOT_MODIFIED);
    let changed_validator = harness
        .request(Method::GET, &url)
        .header(header::RANGE, "bytes=0-1")
        .header(header::IF_RANGE, "\"elsewhere\"")
        .body(Body::empty())
        .unwrap();
    assert_eq!(harness.send(changed_validator).await.status, StatusCode::OK);
}

#[tokio::test]
async fn a_replaced_file_invalidates_old_links() {
    let harness = Harness::new("").await;
    let old = put(&harness, "output/clip.mp4", &bytes());
    std::thread::sleep(Duration::from_millis(5));
    let new = put(&harness, "output/clip.mp4", b"a different render");
    assert_ne!(old, new);
    let stale = harness
        .get(&format!("/api/files/output/clip.mp4?v={old}"))
        .await;
    assert_eq!(stale.error(StatusCode::GONE), "artifact_changed");
    assert_eq!(stale.json()["error"]["data"]["current_version"], new);
}

#[tokio::test]
async fn types_and_dispositions_follow_the_extension() {
    let harness = Harness::new("").await;
    put(&harness, "captions/en.vtt", b"WEBVTT\n");
    put(&harness, "docs/paper.pdf", b"%PDF-1.4");
    put(&harness, "assets/figure.svg", b"<svg/>");
    put(&harness, "output/clip.mp4", &bytes());
    let vtt = harness.get("/api/files/captions/en.vtt").await;
    assert_eq!(vtt.header(header::CONTENT_TYPE), "text/vtt; charset=utf-8");
    let pdf = harness.get("/api/files/docs/paper.pdf").await;
    assert!(pdf
        .header(header::CONTENT_DISPOSITION)
        .starts_with("attachment;"));
    let svg = harness.get("/api/files/assets/figure.svg").await;
    assert_eq!(svg.header(header::CONTENT_TYPE), "image/svg+xml");
    assert!(svg
        .header(header::CONTENT_SECURITY_POLICY)
        .contains("sandbox"));
    let download = harness.get("/api/files/output/clip.mp4?download=1").await;
    assert_eq!(
        download.header(header::CONTENT_DISPOSITION),
        "attachment; filename=\"clip.mp4\"; filename*=UTF-8''clip.mp4"
    );
    put(&harness, "output/Re cur.mp4", &bytes());
    let encoded = harness.get("/api/files/output/Re%20cur.mp4").await;
    assert_eq!(encoded.status, StatusCode::OK);
}

#[tokio::test]
async fn engine_state_media_cache_and_escapes_are_never_served() {
    let harness = Harness::new("").await;
    put(&harness, ".manim-director/media/videos/x.mp4", &bytes());
    put(
        &harness,
        ".manim-director/artifacts/7f/Recurrence.mp4",
        &bytes(),
    );
    put(&harness, "scenes/secret.key", b"k");
    let artifact = harness
        .get("/api/files/.manim-director/artifacts/7f/Recurrence.mp4")
        .await;
    assert_eq!(artifact.status, StatusCode::OK);
    for (path, reason) in [
        ("/api/files/.manim-director/state.db", "hidden"),
        ("/api/files/.manim-director/media/videos/x.mp4", "hidden"),
        ("/api/files/scenes/..%2F..%2Fetc/passwd.txt", "absolute"),
        ("/api/files/.git/config.txt", "hidden"),
    ] {
        let reply = harness.get(path).await;
        assert_eq!(
            reply.error(StatusCode::BAD_REQUEST),
            "invalid_path",
            "{path}"
        );
        assert_eq!(reply.json()["error"]["data"]["reason"], reason, "{path}");
    }
    let unsupported = harness.get("/api/files/scenes/secret.key").await;
    assert_eq!(
        unsupported.error(StatusCode::BAD_REQUEST),
        "unsupported_file_type"
    );
    let missing = harness.get("/api/files/output/none.mp4").await;
    assert_eq!(missing.error(StatusCode::NOT_FOUND), "not_found");
    assert_eq!(missing.json()["error"]["data"]["resource"], "file");

    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("leak.mp4"), b"outside").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            outside.path().join("leak.mp4"),
            harness.root.join("leak.mp4"),
        )
        .unwrap();
        let escape = harness.get("/api/files/leak.mp4").await;
        assert_eq!(escape.error(StatusCode::BAD_REQUEST), "invalid_path");
        assert_eq!(escape.json()["error"]["data"]["reason"], "outside_project");
        fs::create_dir_all(harness.root.join("output")).unwrap();
        std::os::unix::fs::symlink(
            harness.root.join(".manim-director/state.db"),
            harness.root.join("output/state.mp4"),
        )
        .unwrap();
        let disguised = harness.get("/api/files/output/state.mp4").await;
        assert_eq!(disguised.error(StatusCode::BAD_REQUEST), "invalid_path");
    }

    let media = Harness::new("  media_dir: render-cache\n").await;
    put(&media, "render-cache/videos/x.mp4", &bytes());
    let denied = media.get("/api/files/render-cache/videos/x.mp4").await;
    assert_eq!(denied.error(StatusCode::BAD_REQUEST), "invalid_path");
    assert_eq!(denied.json()["error"]["data"]["reason"], "denied");
}
