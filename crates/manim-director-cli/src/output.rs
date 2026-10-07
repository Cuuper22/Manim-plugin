//! Human and `--json` rendering of command results.

use manim_director_core::{
    ErrorBody, Finding, InitResult, JobRecord, OperationResult, QaStatus, Severity,
};
use manim_director_engine::Inspect;
use serde::Serialize;

pub fn json(value: &impl Serialize) {
    match serde_json::to_string(value) {
        Ok(text) => println!("{text}"),
        Err(error) => eprintln!("error: could not encode the output: {error}"),
    }
}

pub fn job(job: &JobRecord, machine: bool) {
    if machine {
        return json(job);
    }
    let cached = if job.cached { " (cached)" } else { "" };
    println!("{} {} {}{cached}", job.operation, job.status, job.id);
    if let Some(result) = &job.result {
        summary(result);
        for artifact in result.artifacts() {
            println!("  {}", artifact.path);
        }
    }
    if let Some(error) = &job.error {
        failure(error);
    }
}

fn summary(result: &OperationResult) {
    match result {
        OperationResult::Doctor(doctor) => {
            println!(
                "  ready to render: {}",
                if doctor.ok { "yes" } else { "no" }
            );
            findings(&doctor.findings);
        }
        OperationResult::Render(render) => println!(
            "  {} ({}): {:.1} s, {} animations",
            render.scene.name, render.scene.file, render.duration_seconds, render.animations
        ),
        OperationResult::Qa(qa) => {
            let status = match qa.status {
                QaStatus::Pass => "pass",
                QaStatus::Warn => "warn",
                QaStatus::Fail => "fail",
            };
            println!("  {status}");
            findings(&qa.findings);
        }
        OperationResult::Diagnose(diagnosis) => findings(&diagnosis.findings),
        OperationResult::ValidateMath(math) => {
            let verdict = match math.valid {
                Some(true) => "every step is equivalent",
                Some(false) => "a step is not equivalent",
                None => "undecided",
            };
            println!("  {verdict}");
            for pair in &math.pairs {
                if pair.equivalent != Some(true) {
                    println!(
                        "  steps {} → {}: {:?}",
                        pair.index + 1,
                        pair.index + 2,
                        pair.equivalent
                    );
                }
            }
        }
        OperationResult::Captions(captions) => {
            println!(
                "  {} cues, {:.1} s",
                captions.cue_count, captions.duration_seconds
            );
            findings(&captions.findings);
        }
        _ => {}
    }
}

pub fn failure(error: &ErrorBody) {
    println!("  {}: {}", error.code, error.message);
    let nested = error
        .data
        .as_ref()
        .and_then(|data| data.get("findings"))
        .and_then(|findings| serde_json::from_value::<Vec<Finding>>(findings.clone()).ok());
    if let Some(nested) = nested {
        findings(&nested);
    }
}

fn findings(findings: &[Finding]) {
    for finding in findings {
        let severity = match finding.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        };
        let location = finding
            .location
            .as_ref()
            .map(|location| format!("{}:{}: ", location.file, location.line))
            .unwrap_or_default();
        println!("  {severity} {location}{}", finding.message);
        if let Some(hint) = &finding.hint {
            println!("    hint: {hint}");
        }
    }
}

pub fn init(result: &InitResult, machine: bool) {
    if machine {
        return json(result);
    }
    match &result.name {
        Some(name) => println!(
            "Created {name} with {} in {}",
            result.scene.name, result.scene.file
        ),
        None => println!("Added {} in {}", result.scene.name, result.scene.file),
    }
    for artifact in &result.artifacts {
        println!("  {}", artifact.path);
    }
}

pub fn inspect(summary: &Inspect, machine: bool) {
    if machine {
        return json(summary);
    }
    println!("{} ({})", summary.name, summary.root.display());
    println!(
        "  profiles: {} (default {})",
        summary.profiles.join(", "),
        summary.default_profile
    );
    if let Some(theme) = &summary.theme {
        println!("  theme: {theme}");
    }
    for scene in &summary.scenes {
        let latest = summary
            .latest
            .iter()
            .find(|latest| latest.scene_id == scene.scene_id)
            .and_then(|latest| latest.video.as_deref())
            .unwrap_or("not rendered");
        println!(
            "  {} {}:{} ({} beats) — {latest}",
            scene.name, scene.file, scene.line, scene.beats
        );
    }
    findings(&summary.findings);
    for job in &summary.recent_jobs {
        println!(
            "  {} {} {} {}",
            job.created_at, job.operation, job.status, job.id
        );
    }
}
