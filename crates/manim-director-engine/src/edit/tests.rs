use super::*;
use manim_director_core::DiscoveredScene;
use serde_json::json;
use std::sync::{Arc, Barrier};

struct Project {
    _directory: tempfile::TempDir,
    root: PathBuf,
}

impl Project {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        Self {
            _directory: directory,
            root,
        }
    }

    fn file(&self, path: &str, content: &[u8]) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.root.join(path)).unwrap()
    }

    fn write(
        &self,
        path: &str,
        expected: Option<String>,
        edit: SourceEdit,
    ) -> Result<SourceWriteResult, EngineError> {
        let write = SourceWrite {
            path: path.into(),
            expected_revision: expected,
            edit,
        };
        write_source(&self.root, Path::new("python3"), &write, None)
    }

    fn revision(&self, path: &str) -> Option<String> {
        current_revision(&self.root, path).unwrap()
    }
}

fn replace_all(content: &str) -> SourceEdit {
    SourceEdit::ReplaceAll {
        content: content.into(),
    }
}

fn lines(start_line: u64, end_line: u64, replacement: &str) -> SourceEdit {
    SourceEdit::ReplaceLines {
        start_line,
        end_line,
        replacement: replacement.into(),
    }
}

fn python_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Reassembles a file from 2-line pages the way the workbench does.
fn reassemble(project: &Project, path: &str) -> (String, SourcePage) {
    let mut pieces = Vec::new();
    let mut start = 1;
    loop {
        let page = read_source(&project.root, path, Some(start), Some(start + 1)).unwrap();
        pieces.push(page.content.clone());
        if page.end_line >= page.total_lines {
            let mut full = pieces.join("\n");
            if page.final_newline {
                full.push('\n');
            }
            return (full, page);
        }
        start = page.end_line + 1;
    }
}

#[test]
fn pages_reassemble_byte_exactly_and_replace_all_writes_verbatim() {
    let project = Project::new();
    for (content, eol, total) in [
        ("a\r\nb\r\n\r\n", Eol::Crlf, 3),
        ("x", Eol::Lf, 1),
        ("a\n\n\nb\n", Eol::Lf, 4),
        ("mixed\r\nends\n", Eol::Lf, 2),
    ] {
        project.file("notes.txt", content.as_bytes());
        let (full, page) = reassemble(&project, "notes.txt");
        assert_eq!(full, content);
        assert_eq!((page.eol, page.total_lines), (eol, total), "{content:?}");
        assert_eq!(page.bytes, content.len() as u64);
        let saved = project
            .write("notes.txt", Some(page.revision), replace_all(&full))
            .unwrap();
        assert_eq!(project.read("notes.txt"), content);
        assert_eq!(saved.total_lines, total);
    }
}

#[test]
fn empty_files_and_page_bounds_follow_the_line_model() {
    let project = Project::new();
    project.file("empty.md", b"");
    let page = read_source(&project.root, "empty.md", None, None).unwrap();
    assert_eq!(
        (
            page.start_line,
            page.end_line,
            page.total_lines,
            page.content.as_str()
        ),
        (1, 0, 0, "")
    );
    project.file("three.md", b"1\n2\n3\n");
    let clamped = read_source(&project.root, "three.md", Some(2), Some(99)).unwrap();
    assert_eq!((clamped.end_line, clamped.content.as_str()), (3, "2\n3"));
    for (start, end) in [(0, 1), (4, 4), (3, 1)] {
        assert!(matches!(
            read_source(&project.root, "three.md", Some(start), Some(end)),
            Err(EngineError::LineOutOfRange { total_lines: 3, .. })
        ));
    }
    assert!(matches!(
        read_source(&project.root, "missing.md", None, None),
        Err(EngineError::NotFound {
            resource: Resource::File,
            ..
        })
    ));
}

#[test]
fn line_edits_keep_the_files_line_ending_and_final_newline() {
    assert_eq!(
        replace_lines("a\nb\nc\n", 2, 2, "x\ny").unwrap(),
        "a\nx\ny\nc\n"
    );
    assert_eq!(replace_lines("a\nb", 3, 2, "c\n").unwrap(), "a\nb\nc");
    assert_eq!(replace_lines("a\nb\n", 1, 0, "z").unwrap(), "z\na\nb\n");
    assert_eq!(replace_lines("a\nb\n", 1, 2, "").unwrap(), "");
    assert_eq!(
        replace_lines("a\r\nb\r\n", 2, 2, "x\r\ny").unwrap(),
        "a\r\nx\r\ny\r\n"
    );
    assert_eq!(replace_lines("", 1, 0, "first").unwrap(), "first");

    let project = Project::new();
    project.file("proof.tex", b"a\r\nb\r\n");
    let result = project
        .write(
            "proof.tex",
            project.revision("proof.tex"),
            lines(2, 2, "c\nd"),
        )
        .unwrap();
    assert_eq!(project.read("proof.tex"), "a\r\nc\r\nd\r\n");
    assert_eq!(result.total_lines, 3);
    assert!(matches!(
        project.write("proof.tex", project.revision("proof.tex"), lines(9, 9, "x")),
        Err(EngineError::LineOutOfRange { total_lines: 3, .. })
    ));
    assert!(matches!(
        replace_lines("a\n", 3, 3, "x"),
        Err(EngineError::LineOutOfRange {
            start_line: 3,
            end_line: 3,
            total_lines: 1
        })
    ));
}

#[test]
fn stale_or_missing_revisions_conflict() {
    let project = Project::new();
    project.file("notes.md", b"one\n");
    let stale = project.revision("notes.md");
    project
        .write("notes.md", stale.clone(), replace_all("two\n"))
        .unwrap();
    let error = project
        .write("notes.md", stale.clone(), replace_all("three\n"))
        .unwrap_err();
    let current = project.revision("notes.md");
    assert_eq!(
        error,
        EngineError::RevisionConflict {
            path: "notes.md".into(),
            expected_revision: stale,
            current_revision: current.clone(),
        }
    );
    assert_eq!(error.status(), 409);
    assert!(matches!(
        project.write("notes.md", None, replace_all("x")),
        Err(EngineError::RevisionConflict {
            current_revision: Some(_),
            ..
        })
    ));
    assert!(matches!(
        project.write("gone.md", current, replace_all("x")),
        Err(EngineError::RevisionConflict {
            current_revision: None,
            ..
        })
    ));
    assert_eq!(project.read("notes.md"), "two\n");
}

#[test]
fn concurrent_writers_from_one_revision_cannot_both_win() {
    let project = Arc::new(Project::new());
    project.file("notes.txt", b"base\n");
    let base = project.revision("notes.txt");
    let writers = 8;
    let barrier = Arc::new(Barrier::new(writers));
    let handles: Vec<_> = (0..writers)
        .map(|index| {
            let (project, barrier, base) = (project.clone(), barrier.clone(), base.clone());
            std::thread::spawn(move || {
                barrier.wait();
                project.write("notes.txt", base, replace_all(&format!("writer {index}\n")))
            })
        })
        .collect();
    let outcomes: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    let winners: Vec<_> = outcomes
        .iter()
        .filter_map(|outcome| outcome.as_ref().ok())
        .collect();
    assert_eq!(winners.len(), 1);
    for error in outcomes.iter().filter_map(|outcome| outcome.as_ref().err()) {
        assert!(
            matches!(error, EngineError::RevisionConflict { .. }),
            "{error:?}"
        );
    }
    assert_eq!(
        project.revision("notes.txt"),
        Some(winners[0].revision.clone())
    );
}

#[test]
fn creation_makes_parents_and_overwrites_leave_an_undo_snapshot() {
    let project = Project::new();
    let created = project
        .write("scenes/new/proof.tex", None, replace_all("\\alpha\n"))
        .unwrap();
    assert_eq!(created.previous_revision, None);
    assert_eq!(project.read("scenes/new/proof.tex"), "\\alpha\n");
    assert!(!project.root.join(UNDO_DIR).exists(), "nothing to snapshot");
    project
        .write(
            "scenes/new/proof.tex",
            Some(created.revision),
            replace_all("\\beta\n"),
        )
        .unwrap();
    let snapshots: Vec<_> = fs::read_dir(project.root.join(UNDO_DIR))
        .unwrap()
        .map(|entry| entry.unwrap().path().join("scenes/new/proof.tex"))
        .collect();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(fs::read_to_string(&snapshots[0]).unwrap(), "\\alpha\n");
}

#[test]
fn paths_are_rejected_before_anything_is_created() {
    let project = Project::new();
    let invalid = |path: &str| match project.write(path, None, replace_all("x")) {
        Err(EngineError::InvalidPath { reason, .. }) => reason,
        other => panic!("{path}: {other:?}"),
    };
    assert_eq!(invalid(".github/x.yml"), "hidden");
    assert_eq!(invalid(".manim-director/state.json"), "hidden");
    assert_eq!(invalid("../x.py"), "traversal");
    assert_eq!(invalid("/etc/x.py"), "absolute");
    assert!(matches!(
        project.write("tool.sh", None, replace_all("x")),
        Err(EngineError::UnsupportedFileType { .. })
    ));
    fs::create_dir_all(project.root.join("folder.md")).unwrap();
    assert_eq!(invalid("folder.md"), "denied");
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), project.root.join("escape")).unwrap();
        assert_eq!(invalid("escape/deep/new/x.md"), "outside_project");
        assert!(
            !outside.path().join("deep").exists(),
            "no directory is created through the symlink"
        );
        fs::write(outside.path().join("secret.md"), "secret").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.md"),
            project.root.join("linked.md"),
        )
        .unwrap();
        assert!(matches!(
            read_source(&project.root, "linked.md", None, None),
            Err(EngineError::InvalidPath {
                reason: "outside_project",
                ..
            })
        ));
    }
}

#[test]
fn size_encoding_json_and_spec_are_validated() {
    let project = Project::new();
    let big = "x".repeat(MAX_SOURCE_BYTES as usize + 1);
    assert!(matches!(
        project.write("big.txt", None, replace_all(&big)),
        Err(EngineError::FileTooLarge { .. })
    ));
    project.file("latin1.txt", &[0xE9, b'\n']);
    assert!(matches!(
        read_source(&project.root, "latin1.txt", None, None),
        Err(EngineError::NotUtf8 { .. })
    ));
    match project.write("data.json", None, replace_all("{\n  \"a\": ,\n}")) {
        Err(EngineError::SourceInvalid { language, line, .. }) => {
            assert_eq!((language.as_str(), line), ("json", Some(2)));
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        project.write(
            SPEC_FILE,
            None,
            replace_all("version: 2\nproject:\n  name: x\n")
        ),
        Err(EngineError::SourceInvalid { .. })
    ));
    assert!(matches!(
        project.write(
            "notes.md",
            None,
            SourceEdit::MergePatch { patch: Map::new() }
        ),
        Err(EngineError::InvalidParams { .. })
    ));
}

#[test]
fn merge_patch_updates_the_spec() {
    let project = Project::new();
    project.file(
        SPEC_FILE,
        b"version: 1\nproject:\n  name: Demo\n  seed: 3\n",
    );
    let patch = json!({"project": {"seed": null, "title": "Recurrences"}});
    let Value::Object(patch) = patch else {
        unreachable!()
    };
    project
        .write(
            SPEC_FILE,
            project.revision(SPEC_FILE),
            SourceEdit::MergePatch { patch },
        )
        .unwrap();
    let spec = DirectorSpec::parse(&project.read(SPEC_FILE)).unwrap();
    assert_eq!(spec.project.title.as_deref(), Some("Recurrences"));
    assert_eq!(spec.project.seed, None);
}

#[test]
fn python_is_checked_with_the_configured_interpreter() {
    let project = Project::new();
    let missing = SourceWrite {
        path: "scenes/a.py".into(),
        expected_revision: None,
        edit: replace_all("x = 1\n"),
    };
    let error = write_source(
        &project.root,
        Path::new("/nonexistent/python"),
        &missing,
        None,
    )
    .unwrap_err();
    assert!(error.to_string().contains("/nonexistent/python"), "{error}");
    if !python_available() {
        eprintln!("skipping the syntax check: python3 is not installed");
        return;
    }
    match project.write("scenes/a.py", None, replace_all("x = 1\ndef broken(:\n")) {
        Err(EngineError::SourceInvalid { line, language, .. }) => {
            assert_eq!((language.as_str(), line), ("python", Some(2)));
        }
        other => panic!("{other:?}"),
    }
    let index = DiscoverResult {
        files: 1,
        truncated: false,
        scenes: vec![DiscoveredScene {
            name: "Intro".into(),
            file: "scenes/a.py".into(),
            line: 1,
            end_line: 2,
            construct_line: None,
            bases: vec!["Scene".into()],
            doc: None,
            theme: None,
            sections: vec![],
            beats: vec![],
        }],
        findings: vec![],
        artifacts: vec![],
    };
    let write = SourceWrite {
        path: "scenes/a.py".into(),
        expected_revision: None,
        edit: replace_all("print('\\d')\n"),
    };
    let result = write_source(&project.root, Path::new("python3"), &write, Some(&index)).unwrap();
    assert_eq!(result.affected_scenes, ["scenes/a.py#Intro"]);
}

#[test]
fn writes_require_an_explicit_revision_and_known_fields() {
    let parsed: SourceWrite = serde_json::from_value(json!({
        "path": "a.md", "expected_revision": null, "edit": {"kind": "replace_all", "content": "x"}
    }))
    .unwrap();
    assert_eq!(parsed.expected_revision, None);
    for body in [
        json!({"path": "a.md", "edit": {"kind": "replace_all", "content": "x"}}),
        json!({"path": "a.md", "expected_revision": null, "edit": {"kind": "replace_all", "content": "x", "extra": 1}}),
        json!({"path": "a.md", "expected_revision": null, "edit": {"kind": "rewrite"}}),
    ] {
        assert!(
            serde_json::from_value::<SourceWrite>(body.clone()).is_err(),
            "{body}"
        );
    }
}
