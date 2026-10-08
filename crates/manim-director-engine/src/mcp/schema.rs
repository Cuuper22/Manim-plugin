//! The ten MCP tools as `tools/list` advertises them (OPS §1.7). Arguments
//! are the operation's params; job tools add `wait_seconds`.

use manim_director_core::Operation;
use serde_json::{json, Map, Value};

pub(super) const TOOLS: [&str; 10] = [
    "init",
    "inspect",
    "doctor",
    "render",
    "still",
    "contact_sheet",
    "qa",
    "validate_math",
    "submit",
    "job_status",
];

pub(super) fn tool_list() -> Value {
    let scene =
        json!({"type": "string", "description": "Scene class, or a scene id from director.yaml."});
    let profile = json!({"type": "string", "description": "Render profile name; see inspect."});
    let source = json!({
        "description": "The media to read: a succeeded job's output or a project path. Default: the latest render of the scene.",
        "oneOf": [
            {"type": "object", "properties": {"job_id": {"type": "string", "format": "uuid"}}, "required": ["job_id"], "additionalProperties": false},
            {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"], "additionalProperties": false}
        ]
    });
    let tools = [
        (
            "init",
            "Create a project in the server's directory from a template, or add one scene template to the existing project.",
            object(
                json!({
                    "name": {"type": "string", "maxLength": 120},
                    "template": {"type": "string", "description": "Project template; default explainer."},
                    "scene_template": {"type": "string", "description": "Adds this scene template to the existing project instead."},
                    "theme": {"type": "string"},
                    "seed": {"type": "integer", "minimum": 0, "maximum": 2147483647},
                    "force": {"type": "boolean", "default": false, "description": "Overwrite template files in a non-empty directory."}
                }),
                &[],
                false,
            ),
        ),
        (
            "inspect",
            "Project summary: profiles, theme, scenes found in the source with their beats and sections, the latest artifacts per scene, recent jobs.",
            object(json!({}), &[], false),
        ),
        (
            "doctor",
            "Check the Python runtime, Manim, LaTeX and ffmpeg.",
            object(json!({}), &[], true),
        ),
        (
            "render",
            "Render one scene to video, with its beat timeline.",
            object(
                json!({
                    "scene": scene,
                    "file": {"type": "string", "description": "Project-relative .py file holding the scene."},
                    "profile": profile,
                    "sections": {"type": "boolean", "default": false, "description": "Also write one video per Manim section."},
                    "fresh": {"type": "boolean", "default": false, "description": "Render again even when a cached render matches."}
                }),
                &[],
                true,
            ),
        ),
        (
            "still",
            "Render one scene's last frame to PNG.",
            object(
                json!({
                    "scene": scene,
                    "file": {"type": "string", "description": "Project-relative .py file holding the scene."},
                    "profile": profile,
                    "fresh": {"type": "boolean", "default": false}
                }),
                &[],
                true,
            ),
        ),
        (
            "contact_sheet",
            "Lay evenly spaced frames of a rendered video out as one labelled PNG.",
            object(
                json!({
                    "source": source,
                    "scene": scene,
                    "profile": profile,
                    "count": {"type": "integer", "minimum": 1, "maximum": 24, "default": 6},
                    "columns": {"type": "integer", "minimum": 1, "maximum": 8, "default": 3}
                }),
                &[],
                true,
            ),
        ),
        (
            "qa",
            "Check a rendered video or image for blank frames, low contrast and safe-area violations, and a DirectedScene render's pacing (caption reading time, holds after reveals, crowded beats and moments, prediction holds), mapped to beats and source lines. A DirectedScene render also gets beats.png: each beat's settled frame under its audience question.",
            object(
                json!({
                    "source": source,
                    "scene": scene,
                    "profile": profile,
                    "frames": {"type": "integer", "minimum": 1, "maximum": 40, "default": 8}
                }),
                &[],
                true,
            ),
        ),
        (
            "validate_math",
            "Check that consecutive steps of a derivation are equal, symbolically and at sampled points.",
            object(
                json!({
                    "steps": {"type": "array", "items": {"type": "string", "maxLength": 2000}, "minItems": 2, "maxItems": 32},
                    "ranges": {
                        "type": "object",
                        "description": "Sampling interval per variable, e.g. {\"x\": [-3, 3]}.",
                        "additionalProperties": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2}
                    },
                    "samples": {"type": "integer", "minimum": 1, "maximum": 10000, "default": 200},
                    "tolerance": {"type": "number", "exclusiveMinimum": 0, "maximum": 1, "default": 1e-9},
                    "seed": {"type": "integer", "minimum": 0}
                }),
                &["steps"],
                true,
            ),
        ),
        (
            "submit",
            "Submit any job operation by name with its params: frame, diagnose, captions, ingest, export and the operations above.",
            json!({
                "type": "object",
                "properties": {
                    "operation": {
                        "type": "string",
                        "enum": Operation::job_operations().map(Operation::as_str).collect::<Vec<_>>()
                    },
                    "wait_seconds": wait_seconds()
                },
                "required": ["operation"],
                "additionalProperties": true
            }),
        ),
        (
            "job_status",
            "Read a job's status, result and log records after a cursor; optionally cancel it first.",
            object(
                json!({
                    "job_id": {"type": "string", "format": "uuid"},
                    "cancel": {"type": "boolean", "default": false},
                    "cursor": {"type": "string", "description": "next_cursor of the previous call."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 20}
                }),
                &["job_id"],
                true,
            ),
        ),
    ];
    let tools: Vec<Value> = tools
        .into_iter()
        .map(|(name, description, schema)| {
            json!({"name": name, "description": description, "inputSchema": schema})
        })
        .collect();
    json!({ "tools": tools })
}

fn wait_seconds() -> Value {
    json!({
        "type": "integer", "minimum": 0, "maximum": 50, "default": 20,
        "description": "How long to wait for the job to end before answering with its current status."
    })
}

fn object(properties: Value, required: &[&str], job: bool) -> Value {
    let mut properties = match properties {
        Value::Object(properties) => properties,
        _ => Map::new(),
    };
    if job {
        properties.insert("wait_seconds".into(), wait_seconds());
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}
