# Generalized Fibonacci

Every sequence with

$$x_{n+2} = p\,x_{n+1} + q\,x_n$$

in five scenes: a family of examples, recorded data, the companion matrix, its characteristic
roots and a 3D view of an oscillating orbit. `scenes.py` is plain Manim on top of
`DirectedScene`; `director.yaml` sets the theme and the colors of `p`, `q` and `λ` for every
scene at once.

| Scene | What it shows |
|---|---|
| `GeneralizedFibonacci` | The full 38-second cut: family, data, matrix, roots, double root, recap. |
| `SequenceData` | `data/sequences.csv` on a linear scale, then on a log scale where growth is a slope. |
| `CharacteristicRoots` | The characteristic equation from the guess `x_n = λ^n`, and the double-root case. |
| `CompanionMatrix` | The Fibonacci states under `C`, with a camera close-up that pulls back to the trend. |
| `StateOrbit3D` | The oscillator `p = 1, q = −1`: its states wind around the time axis every six steps. |

Render with Manim Director, then look at what you rendered:

```bash
manim-director --project examples/generalized-fibonacci render --profile draft
manim-director --project examples/generalized-fibonacci contact-sheet
manim-director --project examples/generalized-fibonacci render --scene StateOrbit3D
```

Or with plain Manim, from this directory:

```bash
manim -ql scenes.py GeneralizedFibonacci
manim -ql scenes.py CompanionMatrix
```

Check the algebra behind the `roots` beat (`λ₊` solves `λ² = pλ + q`):

```bash
manim-director validate-math "((p + sqrt(p^2 + 4*q))/2)^2" "p*(p + sqrt(p^2 + 4*q))/2 + q" --range q=0:5
```

`captions.vtt` and `narration.json` follow the beats of `GeneralizedFibonacci` (their ids and
times match its render timeline). `expected/outputs.json` lists the deliverables and the commands
that produce them; none of them is checked in.
