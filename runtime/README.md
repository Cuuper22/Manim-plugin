# Manim Director runtime

The Python half of Manim Director: the bridge worker the engine runs operations in, and the
authoring API (`DirectedScene`) that scenes import.

## Authoring

```python
from manim import *
from manim_director_runtime import DirectedScene


class SquaredSum(DirectedScene):
    theme = "paper"  # else director.yaml `theme:`, else "midnight"
    symbols = {"a": "primary", "b": "secondary"}  # consistent colors in every formula

    def construct(self):
        question = self.math(r"(a+b)^2 = ?")
        with self.beat("question", focus=question):
            self.title("Squaring a sum")
            self.place(question)
            self.caption("What happens when a sum is squared?")
        with self.beat("expand"):
            proof = self.derive(
                r"(a+b)^2",
                (r"= (a+b)(a+b)", "definition"),
                (r"= a^2 + 2ab + b^2", "distribute and collect"),
                replaces=question,  # the question morphs into the first step
            )
        self.highlight(proof.lines[-1], "2ab", box=True)
```

- Themes live in `data/themes.json` (`midnight`, `paper`, `chalkboard`, `contrast`); tokens are
  attributes (`self.theme.accent`). During a render plain Manim objects default to the theme's
  foreground and font. `director.yaml` `direction.symbols` sets project-wide symbol colors.
- `text`, `tex` and `math` build themed mobjects; `math` colors symbols token by token (never
  the `x` in `\max`) and splits formulas into atoms that `TransformMatchingTex` can match.
- `place(*mobjects, region=...)` fits objects into a `Region` (`SAFE`, `HEADER`, `CONTENT`,
  `LEFT`, `RIGHT`, `TOP`, `BOTTOM`, `CAPTION`), checking size and overlap before anything
  moves; they enter at the next animation. Placing an on-stage object again glides it there;
  `replaces=` morphs an on-stage object into the new one.
- `with self.beat(id, transition=..., keep=[...], focus=..., hold=...)`: at the first
  animation inside, whatever was not kept or placed again leaves; `continue`, `contrast`,
  `reveal` and `chapter` style the change. Title and caption persist until a chapter.
  `run_time=0` lands on the end state without frames. Under the bridge, beats form the
  render's timeline.
- Math: `derive` (relations aligned, notes beside, matching terms carried between steps;
  `in_place=True` transforms one line), `term(eq, tex)` returns a sub-term's glyphs,
  `highlight`, `tag` numbers equations, `focus`/`unfocus` dim everything else.
- Authoring errors are `CompositionError` (a `DirectorError`) with a message that says what
  to change.

## Bridge

```bash
python -P -m manim_director_runtime bridge [--preload]
```

One process serves exactly one request (protocol v2):

1. It writes a `ready` frame (runtime version, Manim version, theme and template catalog).
   With `--preload` it imports Manim first, so a pre-warmed worker renders without import latency.
2. It reads one JSON request line from stdin:
   `{"protocol":2,"request_id":"…","method":"render","project_root":"/abs/project","params":{…}}`.
3. It writes `progress` and `log` frames, then exactly one `result` or `error` frame, and exits.

Frames go to the original stdout; everything else that writes to stdout (Manim's console, `print`
in a scene) is redirected to stderr. Methods: `init discover doctor render still frame
contact_sheet qa diagnose validate_math captions ingest export`. Each method's `params` is a typed
task parsed strictly at the boundary (`tasks.py`); unknown or missing fields fail with
`invalid_params` naming the field. Rendering runs Manim in-process; the engine owns timeouts and
cancels by killing the worker's process group.

## Development

```bash
pip install -e '.[full,test]'
pytest
ruff check src tests
```

Manim-backed tests render tiny scenes with Cairo and skip when Manim, FFmpeg or LaTeX are missing.
