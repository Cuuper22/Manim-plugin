# Showcase

The scenes behind the animations in the [main README](../../README.md). Each is a short loop
written on `DirectedScene` and the explainer kit, then rendered and exported with the engine.

| Scene | File | What it shows | Kit and devices |
|---|---|---|---|
| `EulerHero` | [`hero.py`](scenes/hero.py) | e<sup>iθ</sup> runs around the unit circle; its parts trace cos θ and sin θ; e<sup>iπ</sup> + 1 = 0 | `always_redraw` on a placed stage, `derive(in_place=True, replaces=)` |
| `RiemannToIntegral` | [`riemann.py`](scenes/riemann.py) | Left sums of x² on [0, 2] close in on 8/3 | `FunctionPlot.riemann`, `refine`, `area`, `Readout` |
| `SecantToTangent` | [`tangent.py`](scenes/tangent.py) | The secant at x = 1 turns into the tangent; its slope settles on 2 | `FunctionPlot.secant`, `Readout`, `pause` |
| `DeterminantIsArea` | [`linear_map.py`](scenes/linear_map.py) | A matrix moves the plane; the unit square's image has area det A = 3 | `VectorGrid.apply`, `unit_square`, `Readout` |
| `CompletingTheSquare` | [`quadratic.py`](scenes/quadratic.py) | The quadratic formula by completing the square | `derive` with notes, symbol colors |
| `TaylorSine` | [`taylor.py`](scenes/taylor.py) | Taylor polynomials bend onto sin x, then the full series | `FunctionPlot`, `place(replaces=)`, `anchor` |

`director.yaml` sets the `midnight` theme and a viewer brief, and shortens the reading pauses under
`qa.pacing`: a README loop is watched again and again, so it needs less time to read than a first
viewing does.

To render one and export it as a GIF, from this folder:

```bash
manim-director render --file scenes/riemann.py --scene RiemannToIntegral --profile preview
manim-director export --format gif --scene RiemannToIntegral --profile preview --gif-width 800 --gif-fps 15
```

The GIFs in [`docs/showcase`](../../docs/showcase) were exported that way, `EulerHero` at
`--gif-width 960 --gif-fps 20`.
