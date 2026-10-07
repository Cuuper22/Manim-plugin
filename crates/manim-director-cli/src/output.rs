//! Human and `--json` rendering of command results.

use manim_director_core::{summary, ErrorBody, Finding, InitResult, JobRecord};
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
        for line in summary::verdict(result) {
            println!("  {line}");
        }
        findings(summary::findings(result));
        for artifact in result.artifacts() {
            println!("  {}", artifact.path);
        }
    }
    if let Some(error) = &job.error {
        failure(error);
    }
}

pub fn failure(error: &ErrorBody) {
    println!("  {}: {}", error.code, error.message);
    findings(&summary::error_findings(error));
}

fn findings(findings: &[Finding]) {
    for line in findings.iter().flat_map(summary::finding_lines) {
        println!("  {line}");
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
