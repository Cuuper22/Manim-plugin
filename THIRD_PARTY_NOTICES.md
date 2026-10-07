# Third-party notices

Manim Director statically links open-source Rust crates and embeds a web workbench built from npm packages. Every release archive carries `THIRD_PARTY_LICENSES.txt`: the license texts of exactly the crates and packages in that platform's binary, as `scripts/third_party_licenses.py` collects them from `Cargo.lock` and `workbench/package-lock.json`.

The Rust crates are available under MIT, Apache-2.0, BSD-3-Clause and Unicode-3.0 terms, several of them also under BSD-2-Clause, BSL-1.0, CC0-1.0, MIT-0 or the Unlicense. The embedded workbench contains only MIT-licensed code: React, React DOM and scheduler (Meta Platforms, Inc. and affiliates), CodeMirror 6, Lezer, style-mod, crelt, w3c-keyname and @marijn/find-cluster-break (Marijn Haverbeke and others), and Vite's module-preload helper (VoidZero Inc. and Vite contributors).

The optional Python/Manim environment is installed separately from PyPI rather than embedded in the release binary. Those packages remain governed by their own distributions and licenses; the exact tested versions are listed in `runtime/constraints-full.txt`.

Manim Director's own source and release binaries are provided under the MIT License in `LICENSE`. The complete corresponding source for this release is available at:

https://github.com/Cuuper22/Manim-plugin/tree/v2.0.0
