# {{name}}

`scenes/main.py` is `{{scene}}`, a finished 9:16 short to reshape into your own: keep its
beats, swap in your mathematics. `director.yaml` holds the theme, the phone-safe margins and the
portrait render profiles.

Draft it, then look at it:

```bash
manim-director render --scene {{scene}} --profile draft
manim-director contact-sheet --scene {{scene}}
```

Final render:

```bash
manim-director render --scene {{scene}} --profile production
```

With plain Manim, `manim.cfg` sets the portrait frame; give drafts the same shape, since `-ql`
would ask for a landscape video:

```bash
manim -r 540,960 scenes/main.py {{scene}}
```
