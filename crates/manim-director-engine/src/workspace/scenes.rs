//! The scene index (HTTP §6.1.5) and the views built on it: scenes in
//! workbench order and the storyboard with its code positions (§6.1.6).

use manim_director_core::{
    scene_key, BeatIntent, DirectorSpec, DiscoverResult, DiscoveredScene, ErrorBody, Finding,
    SceneSpec, SectionMark, Timestamp, Transition,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Discover findings that mean a file did not parse.
const PARSE_FAILURES: [&str; 2] = ["python_syntax", "source_encoding"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexState {
    Indexing,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SceneIndexStatus {
    pub state: IndexState,
    pub indexed_at: Option<Timestamp>,
    pub files: u32,
    pub truncated: bool,
    pub error: Option<ErrorBody>,
}

#[derive(Debug, Clone)]
struct IndexedScene {
    scene: DiscoveredScene,
    parse_failed: bool,
}

/// What `discover` last reported, merged with the last good parse of every
/// file that no longer parses.
#[derive(Debug, Clone, Default)]
pub struct SceneIndex {
    refreshing: bool,
    indexed_at: Option<Timestamp>,
    files: u32,
    truncated: bool,
    findings: Vec<Finding>,
    error: Option<ErrorBody>,
    scenes: Vec<IndexedScene>,
    /// Per file, its scenes from the newest scan in which it parsed.
    last_good: BTreeMap<String, Vec<DiscoveredScene>>,
}

impl SceneIndex {
    pub fn begin_refresh(&mut self) {
        self.refreshing = true;
    }

    /// Applies a finished `discover`. A failure keeps the previous scenes.
    pub fn finish_refresh(&mut self, outcome: Result<DiscoverResult, ErrorBody>) {
        self.refreshing = false;
        let result = match outcome {
            Ok(result) => result,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        let unparsed: BTreeSet<String> = result
            .findings
            .iter()
            .filter(|finding| PARSE_FAILURES.contains(&finding.code.as_str()))
            .filter_map(|finding| finding.location.as_ref())
            .map(|location| location.file.clone())
            .collect();
        let mut last_good: BTreeMap<String, Vec<DiscoveredScene>> = BTreeMap::new();
        for scene in &result.scenes {
            last_good
                .entry(scene.file.clone())
                .or_default()
                .push(scene.clone());
        }
        let mut scenes: Vec<IndexedScene> = result
            .scenes
            .iter()
            .map(|scene| IndexedScene {
                scene: scene.clone(),
                parse_failed: false,
            })
            .collect();
        for file in &unparsed {
            if let Some(previous) = self.last_good.remove(file) {
                scenes.extend(previous.iter().map(|scene| IndexedScene {
                    scene: scene.clone(),
                    parse_failed: true,
                }));
                last_good.insert(file.clone(), previous);
            }
        }
        *self = Self {
            refreshing: false,
            indexed_at: Some(Timestamp::now()),
            files: result.files,
            truncated: result.truncated,
            findings: result.findings,
            error: None,
            scenes,
            last_good,
        };
    }

    pub fn status(&self) -> SceneIndexStatus {
        let state = match (self.refreshing, &self.error, self.indexed_at) {
            (true, _, _) | (false, None, None) => IndexState::Indexing,
            (false, Some(_), _) => IndexState::Failed,
            (false, None, Some(_)) => IndexState::Ready,
        };
        SceneIndexStatus {
            state,
            indexed_at: self.indexed_at,
            files: self.files,
            truncated: self.truncated,
            error: self.error.clone().filter(|_| state == IndexState::Failed),
        }
    }

    /// The newest scan's findings, verbatim.
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// The error of the newest refresh while the index is failed.
    pub fn failure(&self) -> Option<&ErrorBody> {
        self.error.as_ref().filter(|_| !self.refreshing)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LineSpan {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SceneBeat {
    pub name: Option<String>,
    pub span: LineSpan,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeclaredScene {
    pub id: String,
    pub purpose: Option<String>,
    pub duration_seconds: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Scene {
    pub id: String,
    pub class_name: String,
    pub file: String,
    pub span: LineSpan,
    pub construct_line: Option<u32>,
    pub bases: Vec<String>,
    pub theme: Option<String>,
    pub summary: Option<String>,
    pub sections: Vec<SectionMark>,
    pub beats: Vec<SceneBeat>,
    pub declared: Option<DeclaredScene>,
    pub parse_failed: bool,
}

/// Scenes declared in `director.yaml` first, in declared order; the rest
/// by file and line.
pub fn scenes(spec: &DirectorSpec, index: &SceneIndex) -> Vec<Scene> {
    let mut ranked: Vec<(usize, Scene)> = index
        .scenes
        .iter()
        .map(|indexed| {
            let declared = spec
                .scenes
                .iter()
                .position(|entry| declares(entry, &indexed.scene));
            let rank = declared.unwrap_or(spec.scenes.len());
            let entry = declared.map(|position| &spec.scenes[position]);
            (rank, scene(&indexed.scene, entry, indexed.parse_failed))
        })
        .collect();
    ranked.sort_by(|(a_rank, a), (b_rank, b)| {
        (a_rank, &a.file, a.span.start).cmp(&(b_rank, &b.file, b.span.start))
    });
    ranked.into_iter().map(|(_, scene)| scene).collect()
}

fn declares(entry: &SceneSpec, scene: &DiscoveredScene) -> bool {
    let class = entry.class_name.as_deref().unwrap_or(&entry.id);
    class == scene.name && entry.file.as_ref().is_none_or(|file| *file == scene.file)
}

fn scene(found: &DiscoveredScene, entry: Option<&SceneSpec>, parse_failed: bool) -> Scene {
    Scene {
        id: scene_key(&found.file, &found.name),
        class_name: found.name.clone(),
        file: found.file.clone(),
        span: LineSpan {
            start: found.line,
            end: found.end_line,
        },
        construct_line: found.construct_line,
        bases: found.bases.clone(),
        theme: found.theme.clone(),
        summary: found.doc.clone(),
        sections: found.sections.clone(),
        beats: found
            .beats
            .iter()
            .map(|beat| SceneBeat {
                name: beat.id.clone(),
                span: LineSpan {
                    start: beat.line,
                    end: beat.end_line,
                },
            })
            .collect(),
        declared: entry.map(|entry| DeclaredScene {
            id: entry.id.clone(),
            purpose: entry.purpose.clone(),
            duration_seconds: entry.duration_seconds,
        }),
        parse_failed,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StoryboardBeatView {
    pub id: String,
    pub intent: Option<BeatIntent>,
    pub transition: Option<Transition>,
    pub audience_question: Option<String>,
    pub takeaway: Option<String>,
    pub focus: Option<String>,
    pub visual_metaphor: Option<String>,
    pub duration_seconds: Option<f64>,
    /// Cumulative; unknown once an earlier beat has no duration.
    pub start_seconds: Option<f64>,
    pub code: Option<CodePosition>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CodePosition {
    pub scene_id: String,
    pub line: u32,
}

pub fn storyboard(spec: &DirectorSpec, scenes: &[Scene]) -> Vec<StoryboardBeatView> {
    let mut start = Some(0.0);
    spec.storyboard
        .iter()
        .map(|beat| {
            let code = scenes.iter().find_map(|scene| {
                scene
                    .beats
                    .iter()
                    .find(|code| code.name.as_deref() == Some(beat.id.as_str()))
                    .map(|code| CodePosition {
                        scene_id: scene.id.clone(),
                        line: code.span.start,
                    })
            });
            let view = StoryboardBeatView {
                id: beat.id.clone(),
                intent: beat.intent,
                transition: beat.transition,
                audience_question: beat.audience_question.clone(),
                takeaway: beat.takeaway.clone(),
                focus: beat.focus.clone(),
                visual_metaphor: beat.visual_metaphor.clone(),
                duration_seconds: beat.duration,
                start_seconds: start,
                code,
            };
            start = start
                .zip(beat.duration)
                .map(|(start, length)| start + length);
            view
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use manim_director_core::{BeatSpan, SourceLocation};

    fn found(name: &str, file: &str, line: u32) -> DiscoveredScene {
        DiscoveredScene {
            name: name.into(),
            file: file.into(),
            line,
            end_line: line + 9,
            construct_line: Some(line + 1),
            bases: vec!["DirectedScene".into()],
            doc: None,
            theme: None,
            sections: vec![],
            beats: vec![BeatSpan {
                id: Some(format!("{}-beat", name.to_lowercase())),
                line: line + 2,
                end_line: line + 4,
            }],
        }
    }

    fn scan(scenes: Vec<DiscoveredScene>, findings: Vec<Finding>) -> DiscoverResult {
        DiscoverResult {
            files: 2,
            truncated: false,
            scenes,
            findings,
            artifacts: vec![],
        }
    }

    fn syntax_error(file: &str) -> Finding {
        let mut finding = Finding::warning("python_syntax", "invalid syntax");
        finding.location = Some(SourceLocation {
            file: file.into(),
            line: 3,
            column: Some(1),
        });
        finding
    }

    #[test]
    fn a_file_that_stops_parsing_keeps_its_last_good_scenes() {
        let mut index = SceneIndex::default();
        assert_eq!(index.status().state, IndexState::Indexing);
        index.finish_refresh(Ok(scan(
            vec![found("A", "scenes/a.py", 1), found("B", "scenes/b.py", 1)],
            vec![],
        )));
        assert_eq!(index.status().state, IndexState::Ready);
        index.finish_refresh(Ok(scan(
            vec![found("A", "scenes/a.py", 1)],
            vec![syntax_error("scenes/b.py")],
        )));
        let spec = DirectorSpec::defaults();
        let listed = scenes(&spec, &index);
        assert_eq!(listed.len(), 2);
        assert!(!listed[0].parse_failed && listed[1].parse_failed);
        assert_eq!(listed[1].id, "scenes/b.py#B");
        // Still failing on the next scan: the same last good parse is kept.
        index.finish_refresh(Ok(scan(vec![], vec![syntax_error("scenes/b.py")])));
        assert_eq!(scenes(&spec, &index).len(), 1);
    }

    #[test]
    fn a_failed_refresh_keeps_scenes_and_reports_the_error() {
        let mut index = SceneIndex::default();
        index.finish_refresh(Ok(scan(vec![found("A", "scenes/a.py", 1)], vec![])));
        index.begin_refresh();
        assert_eq!(index.status().state, IndexState::Indexing);
        assert!(index.status().error.is_none());
        index.finish_refresh(Err(ErrorBody::new("timeout", "slow", None)));
        let status = index.status();
        assert_eq!(status.state, IndexState::Failed);
        assert_eq!(status.error.unwrap().code, "timeout");
        assert_eq!(scenes(&DirectorSpec::defaults(), &index).len(), 1);
    }

    #[test]
    fn a_first_refresh_that_fails_reads_failed() {
        let mut index = SceneIndex::default();
        index.begin_refresh();
        let unavailable = ErrorBody::new("runtime_unavailable", "Python was not found.", None);
        index.finish_refresh(Err(unavailable.clone()));
        let status = index.status();
        assert_eq!(status.state, IndexState::Failed);
        assert_eq!(status.error, Some(unavailable));
        assert_eq!(status.indexed_at, None);
    }

    #[test]
    fn declared_scenes_lead_and_storyboard_beats_find_their_code() {
        let mut index = SceneIndex::default();
        index.finish_refresh(Ok(scan(
            vec![
                found("Intro", "scenes/a.py", 1),
                found("Proof", "scenes/b.py", 20),
                found("Outro", "scenes/a.py", 40),
            ],
            vec![],
        )));
        let spec = DirectorSpec::parse(
            "version: 1\nproject:\n  name: D\nscenes:\n  - {id: proof, class: Proof, purpose: Prove it}\nstoryboard:\n  - {id: proof-beat, duration: 2}\n  - {id: missing}\n  - {id: intro-beat, duration: 1}\n",
        )
        .unwrap();
        let listed = scenes(&spec, &index);
        let ids: Vec<_> = listed
            .iter()
            .map(|scene| scene.class_name.as_str())
            .collect();
        assert_eq!(ids, ["Proof", "Intro", "Outro"]);
        assert_eq!(
            listed[0].declared.as_ref().unwrap().purpose.as_deref(),
            Some("Prove it")
        );
        let beats = storyboard(&spec, &listed);
        assert_eq!(
            beats[0].code,
            Some(CodePosition {
                scene_id: "scenes/b.py#Proof".into(),
                line: 22
            })
        );
        assert_eq!(beats[1].start_seconds, Some(2.0));
        assert_eq!(beats[1].code, None);
        assert_eq!(beats[2].start_seconds, None);
    }
}
