use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeatIntent {
    Introduce,
    Explain,
    Compare,
    Reveal,
    Prove,
    Recap,
}

/// The authoring API's transition vocabulary; `continuation` is the legacy
/// spelling of `continue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transition {
    #[serde(alias = "continuation")]
    Continue,
    Contrast,
    Reveal,
    Chapter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryboardBeat {
    pub id: String,
    #[serde(default)]
    pub intent: Option<BeatIntent>,
    #[serde(default)]
    pub transition: Option<Transition>,
    #[serde(default)]
    pub audience_question: Option<String>,
    #[serde(default)]
    pub takeaway: Option<String>,
    #[serde(default)]
    pub focus: Option<String>,
    #[serde(default)]
    pub visual_metaphor: Option<String>,
    #[serde(default, alias = "duration_seconds")]
    pub duration: Option<f64>,
}

#[cfg(test)]
mod tests {
    use crate::{DirectorSpec, Transition};

    #[test]
    fn beats_only_require_an_id_and_accept_the_legacy_transition() {
        let spec = DirectorSpec::parse(
            "version: 1\nproject:\n  name: Demo\nstoryboard:\n  - id: hook\n  - id: turn\n    intent: prove\n    transition: continuation\n    duration_seconds: 4.5\n",
        )
        .unwrap();
        assert!(spec.storyboard[0].intent.is_none());
        assert_eq!(spec.storyboard[1].transition, Some(Transition::Continue));
        assert_eq!(spec.storyboard[1].duration, Some(4.5));
        assert_eq!(
            serde_json::to_value(Transition::Continue).unwrap(),
            "continue"
        );
    }

    #[test]
    fn beat_vocabularies_are_closed() {
        let error = DirectorSpec::parse(
            "version: 1\nproject:\n  name: Demo\nstoryboard:\n  - id: hook\n    transition: random_bounce\n",
        )
        .unwrap_err();
        assert!(error.to_string().contains("random_bounce"));
    }
}
