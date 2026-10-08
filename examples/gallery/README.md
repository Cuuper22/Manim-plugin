# Explainer gallery

Five short films, one per teaching pattern. Each answers one question a real viewer has, makes
them predict before it shows, lands one "aha", and ends with the question and its answer on
screen together. Copy the folder whose pattern fits your topic, keep its beats and swap in your
mathematics.

| Folder | Pattern | The viewer's question | The aha | Kit and devices |
|---|---|---|---|---|
| [`picture_to_formula`](picture_to_formula) | Build the formula from the picture | Why do 1, 1+3, 1+3+5 keep landing on squares? | Each odd number is an L that wraps a k×k square into (k+1)×(k+1) | `DotArray`, `show`, `link`, `annotate` braces, `derive`, `ask` |
| [`zoom_detail`](zoom_detail) | Zoom in on a detail | Why is sin(0.01) almost exactly 0.01? | Up close, sin x and y = x lie on top of each other | `FunctionPlot.inset`, `secant`, `Readout`, `ask` |
| [`misconception`](misconception) | Misconception, then repair | Does the square root split over a sum? | √a, √b and √(a+b) are the sides of a right triangle | `misconception` card, `Figure`, `annotate`, `link`, `ask` |
| [`contrast`](contrast) | Before and after | A shear tilts every square. Does it change their area? | The tilted square keeps its base and height; a stretch does not | `VectorGrid`, `Readout`, `reserve`, `annotate`, `ask` |
| [`concrete_first`](concrete_first) | Concrete first | You test positive. How worried should you be? | Of the 6 who test positive, only 1 is sick | `DotArray` (a selection lined up), `annotate`, `link`, `focus`, `ask` |

Every film opens on its question as the title and closes on its answer (patterns 1 and 7).
`concrete_first` is paced for an `intro` viewer; the others for `general`.

## How each one is built

- `director.yaml` holds `brief.viewer` (who watches, what they know, their question, the wrong
  guess, the aha) and a `storyboard` entry per beat: the audience question, the one change, the
  takeaway, what to keep, and the aha. Write these before any code.
- `scenes/main.py` has one beat per storyboard entry and no waits or holds: those, the derive
  pauses, the prediction pause and the final still come from the viewer's level, and every
  device waits until the last change has been read (`self.pause()` does the same before a plain
  `play`). Only the aha's motion gets a `run_time`, 2 s or so, long enough to watch it happen.
- The aha is a method or a single call (`wrap`, `zoom`, `shear`, `show(long_side)`) so the
  recap can play it again, faster. `concrete_first` cannot undo its line-up of the positives, so
  its recap dims the crowd instead and leaves the six and their fraction lit.

## Render, check and look

```bash
manim-director render --project examples/gallery/zoom_detail --scene SinNearZero --profile draft
manim-director qa --project examples/gallery/zoom_detail --scene SinNearZero
```

`qa` reports no findings for any of the five, and writes `beats.png`: one settled frame per beat
under its audience question. Read it as the viewer in `brief.viewer` would, with only the frames.

Each film runs 33–41 s at draft. `runtime/tests/test_gallery.py` renders each one's last frame,
checks its length and storyboard, and runs pacing QA on its beat timeline.
