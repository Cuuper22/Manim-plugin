//! MCP tests against the stub runtime in `tests/fixtures/stub_runtime.py`.

use super::*;
use crate::{BridgeConfig, EngineMode, PrunePolicy, SchedulerConfig};
use std::{fs, path::Path};

const STUB: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/stub_runtime.py"
);

async fn scheduler(root: &Path) -> Scheduler {
    fs::write(
        root.join("director.yaml"),
        "version: 1\nproject:\n  name: Demo\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("scenes")).unwrap();
    fs::write(
        root.join("scenes/main.py"),
        "class Intro(Scene):\n    pass\n\nclass Proof(Scene):\n    pass\n",
    )
    .unwrap();
    Scheduler::open(
        root,
        SchedulerConfig {
            mode: EngineMode::Mcp,
            workers: 2,
            queue_capacity: 8,
            bridge: BridgeConfig {
                python: STUB.into(),
                module: "stub".into(),
            },
            prune: PrunePolicy {
                keep_jobs: 500,
                keep_days: 30,
            },
            prewarm: None,
        },
    )
    .await
    .unwrap()
}

async fn call(scheduler: &Scheduler, name: &str, arguments: Value) -> Value {
    let request = json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call",
        "params": {"name": name, "arguments": arguments}});
    let response = handle(scheduler, request).await.unwrap();
    response["result"].clone()
}

fn text(result: &Value) -> &str {
    result["content"][0]["text"].as_str().unwrap()
}

#[tokio::test]
async fn the_server_lists_exactly_the_ten_catalog_tools_and_no_resources() {
    let directory = tempfile::tempdir().unwrap();
    let scheduler = scheduler(directory.path()).await;
    let initialize = handle(
        &scheduler,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
    )
    .await
    .unwrap();
    assert!(initialize["result"]["capabilities"]
        .get("resources")
        .is_none());
    let listed = handle(
        &scheduler,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    )
    .await
    .unwrap();
    let names: Vec<_> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, schema::TOOLS);
    let resources = handle(
        &scheduler,
        json!({"jsonrpc":"2.0","id":3,"method":"resources/list"}),
    )
    .await
    .unwrap();
    assert_eq!(resources["result"]["resources"], json!([]));
    let unknown = handle(
        &scheduler,
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"project_apply"}}),
    )
    .await
    .unwrap();
    assert_eq!(unknown["error"]["code"], -32602);
    assert!(handle(
        &scheduler,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .await
    .is_none());
    for (invalid, id) in [(json!([1, 2]), Value::Null), (json!({"id": 9}), json!(9))] {
        let answer = handle(&scheduler, invalid).await.unwrap();
        assert_eq!(
            (&answer["error"]["code"], &answer["id"]),
            (&json!(-32600), &id)
        );
    }
}

#[tokio::test]
async fn inspect_counts_the_scenes_discovery_finds() {
    let directory = tempfile::tempdir().unwrap();
    let scheduler = scheduler(directory.path()).await;
    let result = call(&scheduler, "inspect", json!({})).await;
    assert_eq!(result["isError"], false, "{result}");
    let scenes = result["structuredContent"]["scenes"].as_array().unwrap();
    assert_eq!(scenes.len(), 2);
    assert_eq!(scenes[1]["scene_id"], "scenes/main.py#Proof");
    assert!(text(&result).starts_with("Demo: 2 scenes"));
    assert_eq!(
        result["structuredContent"]["theme"], "midnight",
        "no theme in director.yaml: the runtime's default theme"
    );

    let refused = call(&scheduler, "inspect", json!({"deep": true})).await;
    assert_eq!(refused["isError"], true);
    assert_eq!(
        refused["structuredContent"]["error"]["code"],
        "invalid_params"
    );
}

#[tokio::test]
async fn inspect_agrees_with_the_workbench_on_order_and_findings() {
    let directory = tempfile::tempdir().unwrap();
    let scheduler = scheduler(directory.path()).await;
    fs::write(
        scheduler.root().join("director.yaml"),
        "version: 1\nproject:\n  name: Demo\ntheme: nonexistent\nscenes:\n  - {id: Proof, file: scenes/main.py}\n",
    )
    .unwrap();
    let result = call(&scheduler, "inspect", json!({})).await;
    let structured = &result["structuredContent"];
    let scenes: Vec<_> = structured["scenes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|scene| scene["name"].as_str().unwrap())
        .collect();
    assert_eq!(scenes, ["Proof", "Intro"], "declared scenes first");
    let codes: Vec<_> = structured["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| finding["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"unknown_theme"), "{codes:?}");
}

#[tokio::test]
async fn job_tools_wait_for_the_result_and_failures_are_error_results() {
    let directory = tempfile::tempdir().unwrap();
    let scheduler = scheduler(directory.path()).await;
    let done = call(
        &scheduler,
        "submit",
        json!({"operation": "diagnose", "text": "hello", "wait_seconds": 10}),
    )
    .await;
    assert_eq!(done["isError"], false, "{done}");
    let structured = &done["structuredContent"];
    assert_eq!(structured["job"]["status"], "succeeded");
    assert_eq!(structured["job"]["operation"], "diagnose");
    assert_eq!(
        structured["result"]["findings"][0]["message"],
        json!("hello")
    );
    assert_eq!(structured["error"], Value::Null);

    let failed = call(
        &scheduler,
        "submit",
        json!({"operation": "diagnose", "text": "error", "wait_seconds": 10}),
    )
    .await;
    assert_eq!(failed["isError"], true);
    assert_eq!(
        failed["structuredContent"]["error"]["code"],
        "render_failed"
    );
    assert!(text(&failed).contains("render_failed"));

    for (name, arguments, field) in [
        ("render", json!({"scene": "Intro", "speed": 2}), "speed"),
        ("doctor", json!({"wait_seconds": 51}), "wait_seconds"),
        ("submit", json!({"operation": "init"}), ""),
    ] {
        let refused = call(&scheduler, name, arguments).await;
        assert_eq!(refused["isError"], true, "{name}");
        let error = &refused["structuredContent"]["error"];
        if field.is_empty() {
            assert_eq!(error["code"], "operation_not_allowed");
        } else {
            assert_eq!(error["code"], "invalid_params");
            assert!(error["data"]["field"].as_str().unwrap().contains(field));
        }
    }
}

#[tokio::test]
async fn job_answers_say_the_verdict_and_give_absolute_paths() {
    let directory = tempfile::tempdir().unwrap();
    let scheduler = scheduler(directory.path()).await;
    let doctor = call(&scheduler, "doctor", json!({"wait_seconds": 10})).await;
    assert!(text(&doctor).contains("\nready to render: yes"), "{doctor}");
    let diagnosed = call(
        &scheduler,
        "submit",
        json!({"operation": "diagnose", "text": "hello", "wait_seconds": 10}),
    )
    .await;
    assert!(text(&diagnosed).ends_with("\ninfo hello"), "{diagnosed}");
    let failed = call(
        &scheduler,
        "submit",
        json!({"operation": "diagnose", "text": "error", "wait_seconds": 10}),
    )
    .await;
    let cause = "\nerror scenes/main.py:3: name 'x' is not defined";
    assert!(text(&failed).ends_with(cause), "{failed}");

    fs::create_dir_all(scheduler.root().join("captions")).unwrap();
    fs::write(
        scheduler.root().join("captions/en.vtt"),
        "WEBVTT\n\n00:00.000 --> 00:01.000\nHi\n",
    )
    .unwrap();
    let arguments = json!({"operation": "captions", "path": "captions/en.vtt",
        "output": "captions/en.srt", "wait_seconds": 10});
    let captions = call(&scheduler, "submit", arguments).await;
    let path = scheduler
        .root()
        .join("captions/en.srt")
        .display()
        .to_string();
    assert_eq!(captions["structuredContent"]["paths"], json!([path]));
    assert_eq!(text(&captions).lines().nth(1), Some("1 cues, 1.0 s"));
    assert!(text(&captions).ends_with(&path));
}

#[tokio::test]
async fn job_status_cancels_and_pages_events() {
    let directory = tempfile::tempdir().unwrap();
    let scheduler = scheduler(directory.path()).await;
    let queued = call(
        &scheduler,
        "submit",
        json!({"operation": "diagnose", "text": "sleep", "wait_seconds": 0}),
    )
    .await;
    assert_eq!(queued["isError"], false);
    let id = queued["structuredContent"]["job"]["id"].clone();
    assert!(text(&queued).contains("job_status"));

    let cancelled = call(
        &scheduler,
        "job_status",
        json!({"job_id": id, "cancel": true, "wait_seconds": 10}),
    )
    .await;
    assert_eq!(cancelled["isError"], true);
    assert_eq!(cancelled["structuredContent"]["job"]["status"], "cancelled");
    assert_eq!(cancelled["structuredContent"]["error"]["code"], "cancelled");

    let done = call(
        &scheduler,
        "submit",
        json!({"operation": "diagnose", "text": "chatty", "wait_seconds": 10}),
    )
    .await;
    let id = done["structuredContent"]["job"]["id"].clone();
    let first = call(
        &scheduler,
        "job_status",
        json!({"job_id": id, "limit": 100, "wait_seconds": 0}),
    )
    .await;
    let structured = &first["structuredContent"];
    assert_eq!(structured["events"].as_array().unwrap().len(), 100);
    assert!(bound::size(structured) <= 48 * 1024);
    let cursor = structured["next_cursor"].clone();
    let next = call(
        &scheduler,
        "job_status",
        json!({"job_id": id, "cursor": cursor, "limit": 5, "wait_seconds": 0}),
    )
    .await;
    let events = next["structuredContent"]["events"].as_array().unwrap();
    assert_eq!(events.len(), 5);
    assert!(
        events[0]["cursor"]
            .as_str()
            .unwrap()
            .parse::<i64>()
            .unwrap()
            > cursor.as_str().unwrap().parse::<i64>().unwrap()
    );
}

#[tokio::test]
async fn a_waiting_job_tool_does_not_hold_up_other_messages() {
    let directory = tempfile::tempdir().unwrap();
    let scheduler = scheduler(directory.path()).await;
    let (mut client, server_input) = tokio::io::duplex(64 * 1024);
    let (server_output, client_output) = tokio::io::duplex(64 * 1024);
    let served = tokio::spawn({
        let scheduler = scheduler.clone();
        async move { serve(&scheduler, server_input, server_output).await }
    });
    let slow = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "submit",
        "arguments": {"operation": "diagnose", "text": "sleep", "wait_seconds": 3}}});
    let ping = json!({"jsonrpc": "2.0", "id": 2, "method": "ping"});
    for message in [slow, ping] {
        client
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
    }
    drop(client);
    let mut answers = BufReader::new(client_output).lines();
    let first: Value = serde_json::from_str(&answers.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(
        first["id"], 2,
        "the ping is answered while the job tool waits"
    );
    let second: Value = serde_json::from_str(&answers.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(
        second["id"], 1,
        "an answer in flight at end of input is still written"
    );
    assert_eq!(
        second["result"]["structuredContent"]["job"]["status"],
        "running"
    );
    served.await.unwrap().unwrap();
    scheduler.shutdown().await;
}
