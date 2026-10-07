//! Plain-text verdicts for a finished job, shared by the CLI and the MCP text
//! content, so a reader learns what a result concludes and not only that the
//! job succeeded.

use crate::{ErrorBody, Finding, MathPair, OperationResult};

/// What `result` concludes, one line each.
pub fn verdict(result: &OperationResult) -> Vec<String> {
    match result {
        OperationResult::Doctor(doctor) => {
            vec![format!("ready to render: {}", yes_no(doctor.ok))]
        }
        OperationResult::Render(render) => vec![format!(
            "{} ({}): {:.1} s, {} animations",
            render.scene.name, render.scene.file, render.duration_seconds, render.animations
        )],
        OperationResult::Qa(qa) => vec![format!("qa: {}", qa.status)],
        OperationResult::ValidateMath(math) => {
            let overall = match math.valid {
                Some(true) => "every step is equivalent",
                Some(false) => "a step is not equivalent",
                None => "undecided",
            };
            let pairs = math
                .pairs
                .iter()
                .filter(|pair| pair.equivalent != Some(true));
            std::iter::once(overall.to_owned())
                .chain(pairs.map(pair_line))
                .collect()
        }
        OperationResult::Captions(captions) => vec![format!(
            "{} cues, {:.1} s",
            captions.cue_count, captions.duration_seconds
        )],
        _ => Vec::new(),
    }
}

/// The findings a result reports.
pub fn findings(result: &OperationResult) -> &[Finding] {
    match result {
        OperationResult::Doctor(doctor) => &doctor.findings,
        OperationResult::Qa(qa) => &qa.findings,
        OperationResult::Diagnose(diagnosis) => &diagnosis.findings,
        OperationResult::Captions(captions) => &captions.findings,
        _ => &[],
    }
}

/// The findings a failed job's error carries (a render's file:line causes).
pub fn error_findings(error: &ErrorBody) -> Vec<Finding> {
    error
        .data
        .as_ref()
        .and_then(|data| data.get("findings"))
        .and_then(|findings| serde_json::from_value(findings.clone()).ok())
        .unwrap_or_default()
}

/// `warning scenes/main.py:46: message`, then its hint, if any.
pub fn finding_lines(finding: &Finding) -> Vec<String> {
    let location = finding
        .location
        .as_ref()
        .map(|location| format!("{}:{}: ", location.file, location.line))
        .unwrap_or_default();
    let mut lines = vec![format!(
        "{} {location}{}",
        finding.severity, finding.message
    )];
    lines.extend(finding.hint.as_ref().map(|hint| format!("  hint: {hint}")));
    lines
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

/// `steps 1 → 2: not equivalent (a=-4.4: 19 vs 19.4)`.
fn pair_line(pair: &MathPair) -> String {
    let (from, to) = (pair.index + 1, pair.index + 2);
    let verdict = match pair.equivalent {
        Some(true) => "equivalent",
        Some(false) => "not equivalent",
        None => "undecided",
    };
    let witness = pair.numeric.counterexample.as_ref().and_then(|example| {
        let (left, right) = (example.left?, example.right?);
        let values: Vec<f64> = example.variables.values().copied().collect();
        if !(left.is_finite() && right.is_finite() && values.iter().all(|v| v.is_finite())) {
            return None;
        }
        let at: Vec<String> = example
            .variables
            .iter()
            .map(|(name, value)| format!("{name}={}", number(*value)))
            .collect();
        Some(format!(
            " ({}: {} vs {})",
            at.join(", "),
            number(left),
            number(right)
        ))
    });
    format!(
        "steps {from} → {to}: {verdict}{}",
        witness.unwrap_or_default()
    )
}

/// At most four decimals, without trailing zeros.
fn number(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    match text {
        "-0" => "0".to_owned(),
        text => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Counterexample, NumericCheck, SourceLocation, SymbolicCheck, ValidateMathResult};
    use std::collections::BTreeMap;

    #[test]
    fn a_failed_derivation_names_the_step_and_a_witness() {
        let pair = |index, equivalent, counterexample| MathPair {
            index,
            equivalent,
            symbolic: SymbolicCheck {
                available: true,
                equivalent,
                difference: None,
            },
            numeric: NumericCheck {
                samples_valid: 10,
                samples_skipped: 0,
                max_abs_error: None,
                max_rel_error: None,
                counterexample,
            },
        };
        let witness = Counterexample {
            variables: BTreeMap::from([("a".into(), -4.4), ("b".into(), 0.04)]),
            left: Some(19.0),
            right: Some(19.360_000_1),
        };
        let result = OperationResult::ValidateMath(ValidateMathResult {
            valid: Some(false),
            variables: vec!["a".into(), "b".into()],
            pairs: vec![
                pair(0, Some(true), None),
                pair(1, Some(false), Some(witness)),
                pair(2, None, None),
            ],
            artifacts: vec![],
        });
        assert_eq!(
            verdict(&result),
            [
                "a step is not equivalent",
                "steps 2 → 3: not equivalent (a=-4.4, b=0.04: 19 vs 19.36)",
                "steps 3 → 4: undecided",
            ]
        );
    }

    #[test]
    fn findings_read_as_severity_location_message_and_hint() {
        let mut finding = Finding::warning("low_contrast", "Contrast is 2.0:1.");
        assert_eq!(finding_lines(&finding), ["warning Contrast is 2.0:1."]);
        finding.location = Some(SourceLocation {
            file: "scenes/main.py".into(),
            line: 46,
            column: None,
        });
        finding.hint = Some("Use a lighter color.".into());
        assert_eq!(
            finding_lines(&finding),
            [
                "warning scenes/main.py:46: Contrast is 2.0:1.",
                "  hint: Use a lighter color."
            ]
        );
    }
}
