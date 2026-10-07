//! `ingest` resolution: host files checked, classified and given a destination.

use crate::confine::write_target;
use manim_director_core::{
    files, Budget, DirectorSpec, EngineError, IngestParams, IngestTask, IngestTaskSource, Task,
};
use std::path::{Component, Path};

const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;

pub(super) fn task(
    root: &Path,
    spec: &DirectorSpec,
    params: &IngestParams,
) -> Result<Task, EngineError> {
    let mut total = 0_u64;
    let mut sources = Vec::with_capacity(params.sources.len());
    for (index, source) in params.sources.iter().enumerate() {
        let field = format!("sources[{index}].path");
        let path = Path::new(&source.path)
            .canonicalize()
            .ok()
            .filter(|path| path.is_file())
            .ok_or_else(|| EngineError::invalid(field.clone(), "missing"))?;
        if files::is_secret_like(&path) || in_credential_dir(&path) {
            return Err(EngineError::invalid(field, "denied"));
        }
        let bytes = path.metadata().map_err(EngineError::internal)?.len();
        if bytes > MAX_FILE_BYTES {
            return Err(EngineError::BudgetExceeded {
                budget: Budget::IngestFileBytes,
                limit: MAX_FILE_BYTES,
                actual: bytes,
            });
        }
        total += bytes;
        let (kind, is_asset) = files::ingest_kind(&path);
        let destination = if is_asset {
            spec.project.asset_dir.as_str()
        } else {
            "sources"
        };
        sources.push(IngestTaskSource {
            path,
            kind: kind.to_owned(),
            destination_dir: write_target(root, destination)?,
            id: source.id.clone(),
            license: source.license.clone(),
            attribution: source.attribution.clone(),
        });
    }
    if total > MAX_TOTAL_BYTES {
        return Err(EngineError::BudgetExceeded {
            budget: Budget::IngestTotalBytes,
            limit: MAX_TOTAL_BYTES,
            actual: total,
        });
    }
    Ok(Task::Ingest(IngestTask {
        sources,
        normalize: params.normalize,
        force: params.force,
        manifest: write_target(root, "sources/manifest.json")?,
    }))
}

fn in_credential_dir(path: &Path) -> bool {
    let names: Vec<_> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect();
    names
        .iter()
        .any(|name| matches!(*name, ".ssh" | ".gnupg" | ".aws"))
        || names.windows(2).any(|pair| pair == [".config", "gcloud"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use manim_director_core::IngestSource;
    use std::fs;

    #[test]
    fn sources_are_classified_and_credentials_refused() {
        let host = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let root = project.path().canonicalize().unwrap();
        fs::write(
            root.join("director.yaml"),
            "version: 1\nproject:\n  name: Demo\n",
        )
        .unwrap();
        let spec = DirectorSpec::load(&root).unwrap();
        for name in ["notes.md", "logo.svg", "id_rsa"] {
            fs::write(host.path().join(name), "x").unwrap();
        }
        fs::create_dir_all(host.path().join(".ssh")).unwrap();
        fs::write(host.path().join(".ssh/config.txt"), "x").unwrap();
        let params = |name: &str| IngestParams {
            sources: vec![IngestSource {
                path: host.path().join(name).to_string_lossy().into_owned(),
                id: None,
                license: None,
                attribution: None,
            }],
            normalize: false,
            force: false,
        };
        let Task::Ingest(notes) = task(&root, &spec, &params("notes.md")).unwrap() else {
            panic!("expected an ingest task")
        };
        assert_eq!(notes.sources[0].kind, "markdown");
        assert_eq!(notes.sources[0].destination_dir, root.join("sources"));
        let Task::Ingest(logo) = task(&root, &spec, &params("logo.svg")).unwrap() else {
            panic!("expected an ingest task")
        };
        assert_eq!(logo.sources[0].destination_dir, root.join("assets"));
        for denied in ["id_rsa", ".ssh/config.txt"] {
            match task(&root, &spec, &params(denied)).unwrap_err() {
                EngineError::InvalidParams { reason, .. } => assert_eq!(reason, "denied"),
                other => panic!("unexpected {other:?}"),
            }
        }
        assert!(task(&root, &spec, &params("missing.md")).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(host.path(), root.join("assets")).unwrap();
            match task(&root, &spec, &params("logo.svg")).unwrap_err() {
                EngineError::InvalidPath { path, reason } => {
                    assert_eq!((path.as_str(), reason), ("assets", "outside_project"))
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    }
}
