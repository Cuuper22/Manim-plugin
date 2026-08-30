# Third-party notices

Manim Director statically links open-source Rust crates and embeds a compiled web workbench. Exact versions are recorded in `Cargo.lock` and `workbench/package-lock.json`; their source distributions carry the corresponding license texts and attribution.

The Rust dependency set uses SPDX terms including MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, BSL-1.0, CC0-1.0, Unicode-3.0, LGPL-2.1-or-later, MIT-0, and Unlicense. The embedded workbench includes React and React DOM (Copyright Meta Platforms, Inc. and affiliates, MIT), Lucide (Copyright Lucide Contributors, ISC), and build-runtime code whose package licenses are recorded in the npm lockfile (MIT, Apache-2.0, BSD-3-Clause, ISC, or MPL-2.0).

The optional Python/Manim environment is installed separately from PyPI rather than embedded in the release binary. Those packages remain governed by their own distributions and licenses; the exact tested versions are listed in `runtime/constraints-full.txt`.

Manim Director's own source and release binaries are provided under the MIT License in `LICENSE`. The complete corresponding source for this release is available at:

https://github.com/Cuuper22/Manim-plugin/tree/v1.1.0
