# The explainer kit

Read this when a scene needs a graph, countable dots, a grid that a matrix moves, a geometric
figure or a live number, or a device that directs the viewer: a note, a link, a prediction
prompt, a misconception card. Components are plain Manim `VGroup`s, themed and safe to `place()`;
devices are `DirectedScene` methods. Each sample below is the body of `construct` in a file that
starts like this one, and the test suite renders every sample:

```python
from manim import *
from manim_director_runtime import DirectedScene
from manim_director_runtime.kit import DotArray, Figure, FunctionPlot, Readout, VectorGrid, hide, reserve
```

## Four kinds of handle

| Handle | Example | Enters and leaves |
|---|---|---|
| Component | `plot = FunctionPlot(...)` | `self.place(plot, region="left")`; leaves at a beat that does not keep it. |
| Part | `plot.axes`, `grid.i_hat`, `card.claim` | With its component. Target it with `focus`, `annotate`, `link`. |
| Overlay | `plot.tangent(1)`, `grid.unit_square()`, a `Figure` mark made after `place` | `self.show(overlay)`. Redrawn from its component every frame, so it follows glides and rescaling; leaves with it; beat-local unless kept. |
| Verb | `plot.refine(bars)`, `grid.apply(M)`, `dots.paint(...)`, `card.refute()` | `self.play(verb)`: one Animation. |

`place` positions top-level content; `show` reveals what belongs to placed content. `show` on
anything else raises "place() it first".

**Reserve, then reveal.** What arrives later is laid out from the start, invisible, so nothing on
screen moves to make room for it: the dots a `DotArray` leaves out with `shown=`, a `VectorGrid`'s
room for every matrix in `fits`, a card's evidence rows and marks. `reserve(part)` (from the kit)
does it for anything; `self.show(part)` reveals it; `hide(part)` fades it back for a replay.

**Live values.** Coordinates take a number, a `ValueTracker`, or a function read every frame
(`grid.det`). Animate the tracker and everything reading it follows.

**Read, then watch.** Devices and `derive` wait until the last change has been read. A plain
`self.play`, or a `place` in mid-beat, does not: call `self.pause()` before one that follows a
reveal.

## Colors keep one meaning

`color=` takes a theme token or `#RRGGBB`. The kit's defaults give each token one meaning; keep
your own code to it:

| Token | Meaning |
|---|---|
| `foreground` | neutral content, points |
| `muted` | context: axes, grid lines, the crowd, older notes |
| `primary` | the main object, the first curve, î |
| `secondary` | the second object, the second curve, ĵ |
| `accent` | look here now: the newest part, the current note, overlay or link |
| `success` | what a repair changed |

Symbol colors (`symbols`) carry identity, so the kit never recolors them: notes, links and boxes
are drawn behind glyphs. For a term, `highlight(eq, term, color=None, box=True)` does the same.

## Samples

A plot whose secant turns into the tangent while a readout measures its slope:

```python
plot = FunctionPlot(lambda x: x**2, x_range=(-0.5, 2.5), labels=["y = x^2"])
h = ValueTracker(1.0)
slope = Readout("slope", lambda: (plot.f(1 + h.get_value()) - plot.f(1)) / h.get_value())
with self.beat("secant"):
    self.place(plot, region="left")
    self.place(slope, region="right")
    self.caption("Shrink the step: the secant turns into the tangent.")
    self.show(plot.secant(1, h))
    self.pause()
    self.play(h.animate.set_value(0.05), run_time=3)
```

A point with its guides, the tangent there, and an area:

```python
plot = FunctionPlot(np.sin, x_range=(0, 6.3), labels=[r"\sin x"])
with self.beat("point"):
    self.place(plot)
    self.caption("The point P at x = 1.")
    self.show(plot.dot(1, label="P"), plot.guides(1))
with self.beat("slope", keep=[plot]):
    self.caption("The tangent at P.")
    self.show(plot.tangent(1))
with self.beat("area", keep=[plot]):
    self.caption("The area under one arch is 2.")
    self.show(plot.area(0, np.pi))
```

Area as bars that refine in place, then a magnified inset:

```python
plot = FunctionPlot(lambda x: x**2, x_range=(0, 2), labels=["y = x^2"])
bars = plot.riemann(0, 2, 4)
with self.beat("bars"):
    self.place(plot)
    self.caption("Four bars estimate the area under the curve.")
    self.show(bars)
with self.beat("refine", keep=[plot, bars]):
    self.caption("Thinner bars hug the curve more closely.")
    self.play(plot.refine(bars), run_time=1.5)
with self.beat("zoom", keep=[plot], run_time=2):
    self.place(plot, region="left")
    self.place(plot.inset(around=(1, 1), radius=0.2), region="right")
    self.caption("Up close, the curve is almost straight.")
```

Natural frequencies: paint the positives in a crowd, then line them up to count them:

```python
def positive(r, c):
    return (r, c) in {(0, 3), (2, 6), (4, 8), (6, 1), (7, 5), (9, 2)}

crowd = DotArray(10, radius=0.15, gap=0.22)
with self.beat("crowd"):
    self.place(crowd, region="left")
    self.caption("Picture 100 people. The test flags 6 of them.")
    self.pause()
    self.play(crowd.paint(positive, "accent"))
with self.beat("gather", keep=[crowd], run_time=2):
    self.place(*crowd.select(positive), region="right", direction=RIGHT)
    self.caption("Line them up to count them: 6.")
```

A matrix moves the grid; the square on î and ĵ shows what happens to area:

```python
shear = [[1, 1], [0, 1]]
grid = VectorGrid(extent=1, unit=1.3, fits=[shear])
square = grid.unit_square(color="accent", opacity=0.3)
panel = VGroup(grid.matrix(shear), Readout("area", grid.det)).arrange(DOWN, buff=0.6)
with self.beat("square"):
    self.place(grid, region="left")
    self.caption("The square on î and ĵ has area 1.")
    self.show(square)
with self.beat("shear", keep=[grid, square], aha=True):
    self.place(panel, region="right")
    self.caption("The square tilts, but its area stays 1.")
    self.pause()
    self.play(grid.apply(shear), run_time=2.5)
```

A figure whose labels find room by themselves; a mark made after `place` is shown later:

```python
fig = Figure({"A": (0, 0), "B": (4, 0), "C": (0, 3)}, labels=False)
fig.polygon("ABC")
fig.right_angle("CAB")
fig.length("AB", "4")
fig.length("AC", "3")
with self.beat("legs"):
    self.place(fig)
    self.caption("Legs of 3 and 4 at a right angle.")
long_side = fig.length("BC", "5")
with self.beat("hypotenuse", keep=[fig]):
    self.caption("The long side is 5.")
    self.show(long_side, run_time=1.5)
```

Marks are notation a newcomer may not know; name each once, as these captions do:

```python
iso = Figure({"A": (0, 3), "B": (-2, 0), "C": (2, 0)}, labels=False)
iso.polygon("ABC")
iso.ticks("AB", "AC")
with self.beat("sides"):
    self.place(iso)
    self.caption("The ticks mark two equal sides.")
with self.beat("angles", keep=[iso]):
    self.caption("So the two angles at the base are equal too.")
    self.show(iso.angle("ABC", label=r"\beta"), iso.angle("BCA", label=r"\beta"))
```

A note on a sub-term, and a term linked to its part of the picture:

```python
roots = self.math(r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}")
with self.beat("formula"):
    self.place(roots)
    self.annotate(roots, "decides how many real roots", term="b^2 - 4ac")
dots = DotArray(3, 4, radius=0.15, gap=0.25, color="primary")
product = self.math(r"3 \times 4 = 12")
with self.beat("count"):
    self.place(dots, product, direction=RIGHT)
    self.caption("Each row is one of the 4s.")
    self.link(self.term(product, "4"), dots.select(lambda r, c: r == 0))
```

A tempting claim, predicted, tested, struck and repaired, on one card:

```python
with self.beat("predict"):
    card = self.misconception(r"\sqrt{a + b} = \sqrt{a} + \sqrt{b}")
    self.ask("Does the root split over a sum? Decide first.")
with self.beat("test", keep=[card]):
    self.caption("Try a = 9 and b = 16.")
    self.play(card.test(r"\sqrt{9 + 16} = 5", r"\sqrt{9} + \sqrt{16} = 7"), run_time=2)
with self.beat("refute", keep=[card]):
    self.caption("5 is not 7: the shortcut fails.")
    self.play(card.refute())
with self.beat("repair", keep=[card]):
    self.caption("The root of a sum is at most the sum of roots.")
    self.play(card.repair(r"\sqrt{a + b} \le \sqrt{a} + \sqrt{b}"), run_time=2)
```

**Replay the aha.** Write the aha as a method, play it slowly in its beat, and again faster in
the recap after putting the stage back: `grid.reset()`, `dots.hide(...)` or `hide(overlay)`.

```python
class ShearKeepsArea(DirectedScene):
    def shear(self, grid, run_time):
        self.play(grid.apply([[1, 1], [0, 1]]), run_time=run_time)

    def construct(self):
        grid = VectorGrid(extent=1, unit=1.3, fits=[[[1, 1], [0, 1]]])
        with self.beat("shear", aha=True):
            self.place(grid)
            self.caption("A shear slides every row of the grid sideways.")
            self.pause()
            self.shear(grid, run_time=2.5)
        with self.beat("recap", keep=[grid]):
            self.caption("A shear tilts the grid but keeps every area.")
            self.play(grid.reset())
            self.shear(grid, run_time=1.2)
```

## Catalog

Signatures are exact; each example is a line of a rendered sample above or of a gallery film.

| Signature | Example |
|---|---|
| `FunctionPlot(*functions, x_range=(-1, 3), y_range=None, size=(6.0, 4.5), labels=(), colors=('primary', 'secondary', 'foreground'), numbers=True, axis_labels=None, breaks=())` | `plot = FunctionPlot(lambda x: x**2, x_range=(0, 2), labels=["y = x^2"])` |
| `FunctionPlot.f(x, curve=0)` | `plot.f(1)` |
| `FunctionPlot.dot(x, curve=0, *, color='foreground', label=None)` | `plot.dot(1, label="P")` |
| `FunctionPlot.guides(x, curve=0)` | `plot.guides(1)` |
| `FunctionPlot.tangent(x, curve=0, *, length=0.45, color='accent')` | `self.show(plot.tangent(1))` |
| `FunctionPlot.secant(x, h, curve=0, *, color='accent', legs=True)` | `self.show(plot.secant(1, h))` |
| `FunctionPlot.area(a, b, curve=0, *, under=None, color='primary', opacity=0.35)` | `self.show(plot.area(0, np.pi))` |
| `FunctionPlot.riemann(a, b, n, *, rule='left', curve=0, color='primary')` | `bars = plot.riemann(0, 2, 4)` |
| `FunctionPlot.refine(bars, factor=2)` | `self.play(plot.refine(bars), run_time=1.5)` |
| `FunctionPlot.inset(around, radius, *, size=(3.2, 3.2))` | `self.place(plot.inset(around=(1, 1), radius=0.2), region="right")` |
| `Readout(label, value, *, decimals=2, unit=None, color='foreground', width=6)` | `Readout("area", grid.det)` |
| `DotArray(rows, cols=None, *, shown=True, radius=0.1, gap=0.2, color='muted')` | `crowd = DotArray(10, radius=0.15, gap=0.22)` |
| `DotArray.at(r, c)` | `sick = crowd.at(*SICK)` |
| `DotArray.select(where, *, color=None)` | `ring = dots.select(ell(k), color="accent")` |
| `DotArray.paint(where, color)` | `self.play(crowd.paint(positive, "accent"))` |
| `DotArray.hide(where=None)` | `self.play(dots.hide(ell(3)))` |
| `VectorGrid(extent=3, *, unit=0.8, fits=(), ghost=True, basis=True, colors=None)` | `grid = VectorGrid(extent=1, unit=1.3, fits=[shear])` |
| `VectorGrid.vector(coords, *, label=None, color='foreground')` | |
| `VectorGrid.unit_square(*, color='foreground', opacity=0.15)` | `square = grid.unit_square(color="accent", opacity=0.3)` |
| `VectorGrid.matrix(M=None)` | `grid.matrix(shear)` |
| `VectorGrid.det()` | `Readout("area", grid.det)` |
| `VectorGrid.apply(M, **kwargs)` | `self.play(grid.apply(shear), run_time=2.5)` |
| `VectorGrid.reset(**kwargs)` | `self.play(grid.reset())` |
| `Figure(points, *, unit=1.0, labels=True, color='foreground')` | `fig = Figure({"A": (0, 0), "B": (4, 0), "C": (0, 3)}, labels=False)` |
| `Figure.p(name)` | |
| `Figure.segment(ab, *, color='foreground', label=None, side=0)` | `across = triangle.segment("BC")` |
| `Figure.polygon(names, *, fill='muted', opacity=0.25, color='foreground')` | `fig.polygon("ABC")` |
| `Figure.angle(abc, *, label=None, radius=0.45, color='secondary')` | `iso.angle("ABC", label=r"\beta")` |
| `Figure.right_angle(abc, *, size=0.25, color='muted')` | `fig.right_angle("CAB")` |
| `Figure.ticks(*segments, count=1, color='muted')` | `iso.ticks("AB", "AC")` |
| `Figure.length(ab, label, *, side=0, color=None)` | `long_side = fig.length("BC", "5")` |
| `reserve(mobject)` | `sums = VGroup(*done, reserve(VGroup(last, guess)))` |
| `hide(*mobjects, run_time=0.8)` | `self.play(hide(long_side))` |
| `self.show(*mobjects, run_time=None, lag=0.15)` | `self.show(long_side, run_time=1.5)` |
| `self.annotate(target, note, *, term=None, occurrence=None, style='arrow', side='auto', color='accent', mark=True, persist=False, run_time=None)` | `self.annotate(roots, "decides how many real roots", term="b^2 - 4ac")` |
| `self.link(*targets, color='accent', run_time=None, persist=False)` | `self.link(self.term(product, "4"), dots.select(lambda r, c: r == 0))` |
| `self.ask(question, *, hold=None)` | `self.ask("Does the root split over a sum? Decide first.")` |
| `self.misconception(claim, *, tag='Tempting', region='left', evidence=2)` | `card = self.misconception(r"\sqrt{a + b} = \sqrt{a} + \sqrt{b}")` |
| `Misconception.test(*evidence)` | `self.play(card.test(r"\sqrt{9 + 16} = 5", r"\sqrt{9} + \sqrt{16} = 7"), run_time=2)` |
| `Misconception.refute()` | `self.play(card.refute())` |
| `Misconception.repair(fix)` | `self.play(card.repair(r"\sqrt{a + b} \le \sqrt{a} + \sqrt{b}"), run_time=2)` |
| `self.pause()` | `self.pause()` |

`annotate` styles: `arrow` (a note and an arrow), `brace` (along a term's long side) and `label`
(a name set right beside a part, no pointer). In a beat the newest note is lit and older ones turn
muted. A note on a single symbol defines it for `qa`. After `repair`, `card.fix` is the repaired
claim and the struck one stays in view.

## Errors

Misuse raises `CompositionError` before anything moves, with the fix in the message:

| Message starts | Do |
|---|---|
| "… is not placed: place() it first" | `place` the component; `show` only reveals its overlays and reserved parts. |
| "x=… is outside the plot's x_range" | Widen `x_range`, or keep the point inside it. |
| "Curve … is not finite at x=…" | Narrow `x_range`, or split the curve there with `breaks=[x]`. |
| "This matrix moves the grid … outside the room reserved by fits=" | Add the cumulative matrix to `fits`. |
| "No free space beside … for the note" | Shorten the note, give the target room, or pick `side=`. |
| "Cannot link …: not on stage" | Link what is visible in this beat; keep it with `keep=`. |
| "No point 'D' in this figure" | Use a name from the figure's `points`. |
| "test() fills … more evidence row(s)" | Ask for rows: `misconception(..., evidence=3)`. |
| "paint() selected no visible dots" | `show` reserved dots before painting them. |

## Your own components

Subclass `VGroup` and set three attributes so the pacing QA can count what the viewer must take
in: `director_kind` (a name), `director_chunks` (new things to identify, usually 1) and
`director_read` (seconds to take it in). An overlay also sets `director_parent` to its component:
it then follows the component and leaves with it.
