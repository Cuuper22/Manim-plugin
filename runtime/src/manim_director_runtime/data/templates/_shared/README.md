# {{name}}

`scenes/main.py` is `{{scene}}`, a finished scene to reshape into your own: keep its beats,
swap in your mathematics. `director.yaml` holds the theme and the storyboard those beats follow.

Draft it, then look at it:

```bash
manim-director render --scene {{scene}} --profile draft
manim-director contact-sheet --scene {{scene}}
```

Final render:

```bash
manim-director render --scene {{scene}} --profile production
```

Scenes are plain Manim, so `manim -ql scenes/main.py {{scene}}` works too in the environment
Manim Director runs in.
