//! HTTP §3, §6 and §14 items 13–29, 46–47: error envelopes, jobs, source
//! and the workspace snapshot.

use super::*;
use serde_json::json;

#[tokio::test]
async fn framework_rejections_use_the_envelope() {
    let harness = Harness::new("").await;
    let missing = harness.get("/api/nope").await;
    assert_eq!(missing.error(StatusCode::NOT_FOUND), "not_found");
    assert_eq!(
        missing.json()["error"]["data"],
        json!({"resource": "route", "key": "/api/nope"})
    );
    let wrong_method = harness.json(Method::PUT, "/api/jobs", &json!({})).await;
    assert_eq!(
        wrong_method.error(StatusCode::METHOD_NOT_ALLOWED),
        "method_not_allowed"
    );
    let allowed = wrong_method.json()["error"]["data"]["allowed"].clone();
    assert!(
        allowed.as_array().unwrap().contains(&json!("POST")),
        "{allowed}"
    );
    let options = harness
        .send(
            harness
                .request(Method::OPTIONS, "/api/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(options.status, StatusCode::METHOD_NOT_ALLOWED);
    assert!(options.headers.get("access-control-allow-origin").is_none());

    let malformed = harness
        .send(
            harness
                .request(Method::POST, "/api/jobs")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{\"operation\":"))
                .unwrap(),
        )
        .await;
    assert_eq!(malformed.error(StatusCode::BAD_REQUEST), "invalid_params");
    assert_eq!(malformed.json()["error"]["data"]["field"], Value::Null);
    let bad_limit = harness.get("/api/jobs?limit=0").await;
    assert_eq!(bad_limit.error(StatusCode::BAD_REQUEST), "invalid_params");
    assert_eq!(bad_limit.json()["error"]["data"]["field"], "limit");
    let bad_id = harness.get("/api/jobs/not-a-uuid").await;
    assert_eq!(bad_id.error(StatusCode::BAD_REQUEST), "invalid_params");
    assert_eq!(bad_id.json()["error"]["data"]["field"], "id");
    let unknown = harness
        .get("/api/jobs/0b6f2c1e-6a0f-4c2e-9a51-1f7c3d0d2a11")
        .await;
    assert_eq!(unknown.error(StatusCode::NOT_FOUND), "not_found");
    assert_eq!(unknown.json()["error"]["data"]["resource"], "job");
}

#[tokio::test]
async fn submissions_are_typed_operation_requests() {
    let harness = Harness::new("").await;
    let unknown = harness
        .json(Method::POST, "/api/jobs", &json!({"operation": "preview"}))
        .await;
    assert_eq!(unknown.error(StatusCode::BAD_REQUEST), "invalid_params");
    for operation in ["ingest", "init", "discover"] {
        let refused = harness
            .json(Method::POST, "/api/jobs", &json!({"operation": operation}))
            .await;
        assert_eq!(
            refused.error(StatusCode::BAD_REQUEST),
            "operation_not_allowed"
        );
        let allowed = &refused.json()["error"]["data"]["allowed"];
        assert_eq!(allowed.as_array().unwrap().len(), 10, "{allowed}");
        assert!(!allowed.as_array().unwrap().contains(&json!("ingest")));
    }
    let escaping = harness
        .json(
            Method::POST,
            "/api/jobs",
            &json!({"operation": "render", "file": "../outside.py"}),
        )
        .await;
    assert_eq!(escaping.error(StatusCode::BAD_REQUEST), "invalid_params");
    assert_eq!(escaping.json()["error"]["data"]["field"], "file");
    let extra = harness
        .json(
            Method::POST,
            "/api/jobs",
            &json!({"operation": "doctor", "use_cache": true}),
        )
        .await;
    assert_eq!(extra.error(StatusCode::BAD_REQUEST), "invalid_params");
    assert_eq!(extra.json()["error"]["data"]["field"], "use_cache");

    let large = "x".repeat(200 * 1024);
    let accepted = harness
        .json(
            Method::POST,
            "/api/jobs",
            &json!({"operation": "diagnose", "text": large}),
        )
        .await;
    assert_eq!(accepted.status, StatusCode::ACCEPTED);
    let job = accepted.json();
    let id = job["id"].as_str().unwrap();
    assert_eq!(accepted.header(header::LOCATION), format!("/api/jobs/{id}"));
    assert_eq!(job["origin"], "http");
    assert_eq!(job["request"]["operation"], "diagnose");
    for key in [
        "sequence",
        "cached_from",
        "source_job_id",
        "cancel_requested",
        "artifacts_total",
    ] {
        assert!(job.get(key).is_some(), "{key}");
    }
    assert!(job.get("owner").is_none() && job.get("task").is_none());
    let finished = harness.finished(id).await;
    assert_eq!(finished["status"], "succeeded");
    assert_eq!(finished["result"]["recognized"], false);

    let too_large = "x".repeat(300 * 1024);
    let refused = harness
        .json(
            Method::POST,
            "/api/jobs",
            &json!({"operation": "diagnose", "text": too_large}),
        )
        .await;
    assert_eq!(
        refused.error(StatusCode::PAYLOAD_TOO_LARGE),
        "request_too_large"
    );
    assert_eq!(refused.json()["error"]["data"]["limit_bytes"], 262_144);
}

#[tokio::test]
async fn an_invalid_spec_blocks_renders_but_not_doctor() {
    let harness = Harness::new("render:\n  fps: fast\n").await;
    let render = harness
        .json(Method::POST, "/api/jobs", &json!({"operation": "render"}))
        .await;
    assert_eq!(render.error(StatusCode::BAD_REQUEST), "invalid_spec");
    let doctor = harness
        .json(Method::POST, "/api/jobs", &json!({"operation": "doctor"}))
        .await;
    assert_eq!(doctor.status, StatusCode::ACCEPTED);

    let state = harness.get("/api/state").await;
    assert_eq!(state.status, StatusCode::OK);
    let state = state.json();
    assert_eq!(state["spec"]["valid"], false);
    assert_eq!(state["spec"]["error"]["line"], 5);
    let finding = state["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["code"] == "invalid_spec")
        .expect("an invalid_spec finding");
    assert_eq!(
        finding["location"],
        json!({"file": "director.yaml", "line": 5, "column": 8})
    );
    assert_eq!(finding["source"], "spec");
    assert_eq!(finding["id"], "spec:spec:0");
}

#[tokio::test]
async fn jobs_list_page_get_and_cancel() {
    let harness = Harness::new("").await;
    let mut ids = Vec::new();
    for text in ["one", "two", "three"] {
        let reply = harness
            .json(
                Method::POST,
                "/api/jobs",
                &json!({"operation": "diagnose", "text": text}),
            )
            .await;
        ids.push(reply.json()["id"].as_str().unwrap().to_owned());
    }
    for id in &ids {
        harness.finished(id).await;
    }
    let first = harness.get("/api/jobs?limit=2").await.json();
    let listed: Vec<_> = first["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|job| job["id"].clone())
        .collect();
    assert_eq!(listed, [json!(ids[2]), json!(ids[1])]);
    let before = first["next_before"].as_str().unwrap();
    let rest = harness
        .get(&format!("/api/jobs?limit=2&before={before}"))
        .await
        .json();
    assert_eq!(rest["items"][0]["id"], json!(ids[0]));
    assert_eq!(rest["next_before"], Value::Null);

    let logs = harness
        .get(&format!("/api/jobs/{}/logs?limit=1", ids[0]))
        .await
        .json();
    assert_eq!(logs["items"].as_array().unwrap().len(), 1);
    let after = logs["next_after"].as_str().unwrap();
    let more = harness
        .get(&format!(
            "/api/jobs/{}/logs?after={after}&limit=500",
            ids[0]
        ))
        .await
        .json();
    assert!(more["items"][0]["cursor"].as_str().unwrap() > after);

    let terminal = harness
        .json(
            Method::POST,
            &format!("/api/jobs/{}/cancel", ids[0]),
            &json!({}),
        )
        .await;
    assert_eq!(terminal.status, StatusCode::OK);
    assert_eq!(terminal.json()["status"], "succeeded");

    let sleeper = harness
        .json(
            Method::POST,
            "/api/jobs",
            &json!({"operation": "diagnose", "text": "sleep"}),
        )
        .await
        .json();
    let id = sleeper["id"].as_str().unwrap();
    let cancel = harness
        .json(Method::POST, &format!("/api/jobs/{id}/cancel"), &json!({}))
        .await;
    assert_eq!(cancel.status, StatusCode::ACCEPTED);
    assert_eq!(cancel.json()["cancel_requested"], true);
    let cancelled = harness.finished(id).await;
    assert_eq!(cancelled["status"], "cancelled");
    assert_eq!(cancelled["error"]["code"], "cancelled");
    let with_params = harness
        .json(
            Method::POST,
            &format!("/api/jobs/{id}/cancel"),
            &json!({"force": true}),
        )
        .await;
    assert_eq!(with_params.error(StatusCode::BAD_REQUEST), "invalid_params");
}

#[tokio::test]
async fn source_pages_and_writes_are_exact_and_revision_checked() {
    let harness = Harness::new("").await;
    let content = "a\r\nb\r\n\r\n";
    let created = harness
        .json(
            Method::PUT,
            "/api/source",
            &json!({"path": "notes/a.md", "expected_revision": null,
                    "edit": {"kind": "replace_all", "content": content}}),
        )
        .await;
    assert_eq!(created.status, StatusCode::OK, "{:?}", created.body);
    let revision = created.json()["revision"].as_str().unwrap().to_owned();
    let page = harness
        .get("/api/source?path=notes/a.md&start_line=1")
        .await
        .json();
    assert_eq!(page["eol"], "crlf");
    assert_eq!(page["total_lines"], 3);
    assert_eq!(page["revision"], revision);
    let rebuilt = format!("{}\n", page["content"].as_str().unwrap());
    assert_eq!(rebuilt, content);

    let stale = harness
        .json(
            Method::PUT,
            "/api/source",
            &json!({"path": "notes/a.md", "expected_revision": "0000",
                    "edit": {"kind": "replace_all", "content": "x"}}),
        )
        .await;
    assert_eq!(stale.error(StatusCode::CONFLICT), "revision_conflict");
    assert_eq!(stale.json()["error"]["data"]["current_revision"], revision);
    let hidden = harness.get("/api/source?path=.github/x.yml").await;
    assert_eq!(hidden.error(StatusCode::BAD_REQUEST), "invalid_path");
    let bad_line = harness
        .get("/api/source?path=notes/a.md&start_line=x")
        .await;
    assert_eq!(bad_line.error(StatusCode::BAD_REQUEST), "invalid_params");
    assert_eq!(bad_line.json()["error"]["data"]["field"], "start_line");
}

/// Replaces values that differ between runs, keeping the shape.
fn normalize(state: &mut Value) {
    for pointer in [
        "/engine/instance_id",
        "/engine/started_at",
        "/event_cursor",
        "/project/root",
        "/scene_index/indexed_at",
    ] {
        if let Some(value) = state.pointer_mut(pointer) {
            *value = json!("<volatile>");
        }
    }
}

#[tokio::test]
async fn the_snapshot_has_the_contract_shape() {
    let harness = Harness::new(
        "  title: The Demo\ntheme: midnight\nscenes:\n  - {id: main, class: MainScene, purpose: Open}\nstoryboard:\n  - {id: hook, intent: introduce, duration: 2}\n",
    )
    .await;
    let outcome = harness
        .state
        .scheduler
        .discover()
        .await
        .map_err(|error| error.body());
    harness.state.index().lock().finish_refresh(outcome);
    let mut state = harness.get("/api/state").await.json();
    normalize(&mut state);
    let revision = blake3::hash(
        fs::read(harness.root.join("director.yaml"))
            .unwrap()
            .as_slice(),
    )
    .to_hex()
    .to_string();
    let profile = |name: &str, origin: &str, width, height, fps| {
        json!({"name": name, "origin": origin, "width": width, "height": height, "fps": fps,
               "renderer": "cairo", "format": "mp4", "transparent": false,
               "is_default": name == "preview"})
    };
    let expected = json!({
        "engine": {
            "version": env!("CARGO_PKG_VERSION"), "api_version": 2, "instance_id": "<volatile>",
            "started_at": "<volatile>",
            "limits": {"request_body_bytes": 262144, "source_body_bytes": 3145728,
                       "source_file_bytes": 2097152, "source_page_lines": 2000}
        },
        "event_cursor": "<volatile>",
        "jobs": [],
        "jobs_next_before": null,
        "project": {
            "root": "<volatile>", "name": "The Demo", "description": null,
            "spec_file": "director.yaml", "source_dir": "scenes", "asset_dir": "assets",
            "output_dir": "output", "media_dir": ".manim-director/media", "theme": "midnight",
            "default_profile": "preview", "duration_seconds": 2.0,
            "counts": {"sources": 1, "assets": 0, "outputs": 0}
        },
        "spec": {"path": "director.yaml", "valid": true, "revision": revision, "error": null},
        "profiles": [
            profile("draft", "builtin", 854, 480, 15),
            profile("preview", "builtin", 1280, 720, 30),
            profile("production", "builtin", 1920, 1080, 60),
            profile("ultra", "builtin", 3840, 2160, 60),
            profile("custom", "builtin", 1920, 1080, 60),
        ],
        "themes": [{"name": "midnight", "tokens": [{"token": "background", "color": "#0B1020"}],
                    "is_default": true}],
        "scene_index": {"state": "ready", "indexed_at": "<volatile>", "files": 1,
                        "truncated": false, "error": null},
        "scenes": [{
            "id": "scenes/main.py#MainScene", "class_name": "MainScene", "file": "scenes/main.py",
            "span": {"start": 1, "end": 1}, "construct_line": null, "bases": ["Scene"],
            "theme": null, "summary": null, "sections": [], "beats": [],
            "declared": {"id": "main", "purpose": "Open", "duration_seconds": null},
            "parse_failed": false
        }],
        "storyboard": [{
            "id": "hook", "intent": "introduce", "transition": null, "audience_question": null,
            "takeaway": null, "focus": null, "visual_metaphor": null, "duration_seconds": 2.0,
            "start_seconds": 0.0, "code": null
        }],
        "latest": {"scenes/main.py#MainScene": {"video": null, "still": null, "contact_sheet": null}},
        "findings": [],
        "doctor": null
    });
    assert_eq!(state, expected);
}
