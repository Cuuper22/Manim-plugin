use manim_director_core::{files, DirectorSpec, Task};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read},
    path::Path,
};

const CACHE_SCHEMA: &[u8] = b"manim-director-cache-v3\0";

/// A cache key plus the per-file content hashes it was built from, so the
/// scene file's revision comes from the same read.
#[derive(Debug, Clone)]
pub struct Fingerprint {
    pub value: String,
    files: BTreeMap<String, String>,
}

impl Fingerprint {
    /// blake3 hex of a project-relative file that went into the key.
    pub fn file_hash(&self, relative: &str) -> Option<&str> {
        self.files.get(relative).map(String::as_str)
    }
}

/// OPS §1.5: schema ∥ engine version ∥ runtime identity ∥ op ∥ task (minus
/// `out_dir`/`fresh`) ∥ path and content of every relevant project file.
pub fn fingerprint(
    root: &Path,
    spec: &DirectorSpec,
    runtime_identity: &str,
    task: &Task,
) -> io::Result<Fingerprint> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(CACHE_SCHEMA);
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(b"\0");
    hasher.update(runtime_identity.as_bytes());
    hasher.update(b"\0");
    hasher.update(task.operation().as_str().as_bytes());
    hasher.update(b"\0");
    hasher.update(&serde_json::to_vec(&task.cache_identity())?);

    let paths = match task {
        Task::Discover(discover) => discover.files.clone(),
        _ => {
            let mut paths = spec.ignore_set(root).files_under(root);
            paths.retain(|path| files::has_extension(path, files::RENDER_INPUTS));
            paths
        }
    };
    let mut hashes = BTreeMap::new();
    for path in paths {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        hashes.insert(relative, file_revision(&path)?);
    }
    for (relative, hash) in &hashes {
        hasher.update(relative.as_bytes());
        hasher.update(b"\0");
        hasher.update(hash.as_bytes());
    }
    Ok(Fingerprint {
        value: hasher.finalize().to_hex().to_string(),
        files: hashes,
    })
}

/// blake3 hex of a file's bytes.
pub fn file_revision(path: &Path) -> io::Result<String> {
    let mut hasher = blake3::Hasher::new();
    let mut file = File::open(path)?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use manim_director_core::{DiscoverTask, MediaFormat, RenderSettings, Renderer, StillTask};
    use std::fs;

    fn still(out_dir: &str, fresh: bool) -> Task {
        Task::Still(StillTask {
            scene: Some("A".into()),
            files: vec![],
            settings: RenderSettings {
                profile: "draft".into(),
                width: 854,
                height: 480,
                fps: 15,
                renderer: Renderer::Cairo,
                format: MediaFormat::Png,
                transparent: false,
            },
            media_dir: "/m".into(),
            out_dir: out_dir.into(),
            fresh,
        })
    }

    fn project() -> (tempfile::TempDir, DirectorSpec) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("director.yaml"),
            "version: 1\nproject:\n  name: Demo\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("scenes")).unwrap();
        fs::write(dir.path().join("scenes/a.py"), "x=1").unwrap();
        let spec = DirectorSpec::load(dir.path()).unwrap();
        (dir, spec)
    }

    #[test]
    fn renders_key_on_inputs_but_not_outputs_or_job_dirs() {
        let (dir, spec) = project();
        let root = dir.path();
        let first = fingerprint(root, &spec, "py", &still("/a", false)).unwrap();
        assert_eq!(
            first.value,
            fingerprint(root, &spec, "py", &still("/b", true))
                .unwrap()
                .value
        );
        fs::create_dir_all(root.join("output")).unwrap();
        fs::write(root.join("output/a.mp4"), "ignored").unwrap();
        fs::create_dir_all(root.join(".manim-director/artifacts/x")).unwrap();
        fs::write(root.join(".manim-director/artifacts/x/a.png"), "ignored").unwrap();
        assert_eq!(
            first.value,
            fingerprint(root, &spec, "py", &still("/a", false))
                .unwrap()
                .value
        );
        assert!(first.file_hash("scenes/a.py").is_some());

        fs::write(root.join("scenes/a.py"), "x=2").unwrap();
        assert_ne!(
            first.value,
            fingerprint(root, &spec, "py", &still("/a", false))
                .unwrap()
                .value
        );
        fs::write(root.join("manim.cfg"), "[CLI]\nframe_rate = 30").unwrap();
        let with_cfg = fingerprint(root, &spec, "py", &still("/a", false)).unwrap();
        assert!(with_cfg.file_hash("manim.cfg").is_some());
        assert_ne!(
            with_cfg.value,
            fingerprint(root, &spec, "other-runtime", &still("/a", false))
                .unwrap()
                .value
        );
    }

    #[test]
    fn discover_keys_only_on_its_files() {
        let (dir, spec) = project();
        let root = dir.path();
        let task = Task::Discover(DiscoverTask {
            files: vec![root.join("scenes/a.py")],
        });
        let first = fingerprint(root, &spec, "py", &task).unwrap();
        fs::write(root.join("notes.md"), "unrelated").unwrap();
        assert_eq!(
            first.value,
            fingerprint(root, &spec, "py", &task).unwrap().value
        );
        fs::write(root.join("scenes/a.py"), "x=3").unwrap();
        assert_ne!(
            first.value,
            fingerprint(root, &spec, "py", &task).unwrap().value
        );
    }
}
