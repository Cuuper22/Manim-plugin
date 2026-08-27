# Installing Manim Director

The plugin and local engine are separate layers: Codex installs the plugin manifest/skill, while `manim-director` must be available on the host `PATH` for the stdio MCP declaration.

## Install from the repository

```bash
git clone https://github.com/Cuuper22/Manim-plugin.git
cd Manim-plugin
python3 scripts/install.py --with-manim
```

The installer downloads the platform release binary with its embedded workbench, verifies its exact SHA-256 against the release `SHA256SUMS` asset before opening the archive, creates an isolated runtime environment below the selected prefix, and installs the constrained full Manim/visual/math dependency set there. Linux releases are static musl binaries, and the archive includes the project license and third-party notices. The binary discovers that exact interpreter relative to its own prefix, avoiding active-virtualenv and user-site ambiguity. The default prefix is `~/.local`; add `~/.local/bin` to `PATH` if it is not already present.

Use `--prefix /absolute/prefix` for another destination. Use `--from-source` to build the workbench and Rust binary locally; this requires Rust and Node.js. `--from-source` and `--with-manim` can be combined.

Run the environment check from the project you intend to animate:

```bash
manim-director doctor
```

The doctor reports optional native capabilities such as FFmpeg, LaTeX/Typst, fonts, codecs, and OpenGL. Install only the capabilities required by the intended project/output.

## Add the Codex plugin

```bash
codex plugin marketplace add Cuuper22/Manim-plugin
codex plugin add manim-plugin@manim-director
```

Restart or open a fresh Codex thread after installation so the manifest, MCP server, and `$manim-director` skill are discovered together.

## First project

```bash
mkdir recurrence-film
cd recurrence-film
manim-director init --name "Recurrence Film"
manim-director preview --scene MainScene
manim-director open
```

`open` starts the local workbench on the configured port (default `4177`) and opens it in a browser. If that port is occupied, choose another with `--port`. For a stable URL or remote-forwarded development environment, use:

```bash
manim-director serve --port 4177
```

## Uninstalling the local engine

The installer prints the exact binary destination. A complete uninstall removes that binary plus the same prefix's `share/manim-director/venv/` runtime environment. Removing a project’s `.manim-director/` directory discards only generated cache/job state, not source; do it only when those local jobs and cached renders are no longer needed.
