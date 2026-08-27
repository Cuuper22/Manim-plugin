use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let dist = manifest.join("../../workbench/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("embedded_workbench.rs");
    let mut files = Vec::new();
    collect(&dist, &dist, &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut source = String::from("pub fn embedded_asset(path: &str) -> Option<(&'static [u8], &'static str)> {\nmatch path {\n");
    let has_index = files.iter().any(|(relative, _)| relative == "index.html");
    for (relative, absolute) in files {
        source.push_str(&format!(
            "{:?} => Some((include_bytes!({:?}), {:?})),\n",
            relative,
            absolute.to_string_lossy(),
            mime(&relative)
        ));
    }
    if !has_index {
        source
            .push_str("\"index.html\" => Some((FALLBACK_INDEX, \"text/html; charset=utf-8\")),\n");
    }
    source.push_str("_ => None,\n}\n}\n");
    if !has_index {
        source.push_str("const FALLBACK_INDEX: &[u8] = br#\"<!doctype html><meta charset=utf-8><title>Manim Director</title><body style='font:16px system-ui;background:#0b1020;color:#f5f7ff;padding:3rem'><h1>Manim Director</h1><p>The workbench bundle was not present when this binary was built. Rebuild after <code>workbench/dist</code> exists or pass <code>--workbench-dir</code>.</p></body>\"#;\n");
    }
    fs::write(output, source).unwrap();
}

fn collect(root: &Path, directory: &Path, output: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, output);
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            output.push((relative, path.canonicalize().unwrap_or(path)));
        }
    }
}

fn mime(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
    {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    }
}
