# Generalized Fibonacci

This example is both a standalone Manim project and a director project. It computes every displayed term from

\[
x_{n+2}=p x_{n+1}+q x_n
\]

and covers concrete families, CSV-backed plots, the companion matrix, characteristic roots, a repeated-root edge case, responsive layout, a moving 2D camera, and a rotating 3D state orbit.

Render the narrative cut through the plugin:

```bash
manim-director render --project examples/generalized-fibonacci --profile preview
```

Render it directly with Manim CE:

```bash
cd examples/generalized-fibonacci
manim -pql scenes.py GeneralizedFibonacci
```

Render a focused chapter or the 3D orbit:

```bash
manim -pql scenes.py SequenceData
manim -pql scenes.py CompanionMatrix
manim -pql scenes.py CharacteristicRoots
manim -pql --renderer opengl scenes.py StateOrbit3D
```

Select the high-contrast theme without editing source:

```bash
MANIM_DIRECTOR_THEME=high-contrast manim -pql scenes.py GeneralizedFibonacci
```

`director.yaml` is the source of truth for beats and render profiles. `narration.json` and `captions.vtt` share its section IDs; `assets/manifest.json` records the only visual asset; `expected/outputs.json` describes the deliverables rather than pretending unrendered files already exist.
