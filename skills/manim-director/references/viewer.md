# Planning for the viewer

Read this before planning an explainer and when judging its frames: the viewer model, QA's
budgets, a beat template, seven patterns, and a frames-only critique.

## 1. The viewer you are planning for

Once you know an idea, you cannot easily imagine not knowing it. People who tapped a song's rhythm
expected listeners to name half the songs; listeners named 2.5% [15]. You are the tapper. Your
viewer sees only frames and captions and cannot interrupt. They look at whatever moves or is
brightest [9], hold about four new things at once [3], trust what they can count before symbols
[4], and keep their intuition, often wrong, unless they watch it fail [5, 6].

Before each beat, answer as that viewer: What do I believe now? Where do my eyes go? What do I
expect next? What must still be on screen?

Engagement is tension and release: each beat opens a small question and closes the one before
it; curiosity just before a surprising answer makes it stick [7]. A beat answering a question
nobody asked needs a question beat first, or should go.

## 2. The viewer model

Write `brief.viewer` in `director.yaml` before the storyboard. `qa` reads `level`, `knows`,
`question`, `wrong_guess` and `aha`.

| Key | Rule |
|---|---|
| `who` | A person: what they already use, and where they watch (a phone shrinks everything). |
| `level` | `intro`, `general` or `expert`: the budget column in §3 for holds, pauses and QA. Scaffolds that help novices slow experts down [2]. |
| `knows` | What they use without introduction. Notation written `$…$` (`$n^2$`) is exempt from `unexplained_notation`. |
| `new` | Every term and symbol the film introduces. Show each (a picture, a number) before naming it [1]. |
| `question` | Theirs, in their words, using nothing from `new` [7]. It is the title; the last frame answers it. |
| `wrong_guess` | What they would predict, and why it feels right. Mandatory: watching a misconception fail beats a clean exposition, d ≈ 0.8 [6]. |
| `aha` | One frame or motion you could point at. Build the film backward from it. |
| `payoff` | Something they can now do or predict. |
| `colors` | Token → meaning: at most four, one meaning each, for the whole film. |

## 3. Budgets

The numbers behind the default holds and the pacing findings. What a plan feels most, at
`general`:

- A caption stays at least max(1.5 s, 0.5 s + words ÷ 2.5): 4.1 s for 9 words. At most 12
  words [12].
- At most 3 new things per beat: an expression, shape, label or overlay [3].
- 3 s of stillness after `ask` [11]; 2 s after the aha and any reveal, prove or recap beat [10].
- The aha's motion lasts 1.5 s or more; nothing that matters moves in under 0.6 s.
- A beat lasts 4–15 s; the final frame holds 3 s; one idea per film, 45 s to 3 min [13].

Each key by `level` (general, intro, expert). Override one for a project with
`qa.pacing: {key: value}` in `director.yaml`.

```text
words_per_second        2.5   2.0   3.0    caption and text reading rate
caption_min_seconds     1.5   2.0   1.2    shortest time a caption is on screen
read_base_text          0.5   0.7   0.4    text reading: base + words ÷ rate
read_base_math          1.0   1.3   0.8    math reading: base + per group × term groups
read_per_term_group     0.3   0.4   0.25   math reading per term group
glyphs_per_term_group   3     3     3      glyphs read as one term group
morph_base_share        0.5   0.5   0.5    share of the math base a morph needs
read_per_shape          0.5   0.65  0.4    per shape or overlay entering
read_shapes_max         1.0   1.3   0.8    cap for shapes in one event
read_per_value          1.0   1.3   0.8    per live value (a readout) a motion changes
read_max                4.0   5.0   3.2    cap for any one event
settle_min              0.5   0.7   0.4    shortest still after a reveal, mid-beat
beat_end_min            1.0   1.3   0.8    shortest still at a beat's end
result_end_min          2.0   2.5   1.5    the same after an aha, reveal, prove or recap beat
chain_seconds           1.0   1.0   1.0    reveals of one component this close count as one
ask_hold                3.0   3.0   3.0    stillness after ask
final_hold              3.0   3.5   2.5    automatic final still
motion_min              0.6   0.6   0.6    shortest morph or derive step
step_pause_min          0.4   0.5   0.3    shortest pause between derive steps
aha_motion_min          1.5   1.8   1.2    the aha beat's longest motion
rush_glyphs             8     8     8      more glyphs than this changing within rush_seconds is rushed
rush_seconds            1.0   1.0   1.0    see rush_glyphs
max_new_per_beat        3     2     4      new chunks entering in one beat
max_targets_per_motion  2     2     2      independent targets in one animation
split_fraction          0.33  0.33  0.33   targets this far apart (of frame width) are two places
signal_min_chunks       2     2     2      a beat bringing in this many needs a caption, focus or note
max_visible_chunks      7     6     8      lit content chunks at a still
max_colors              4     4     4      meaningful colors at a still
caption_max_words       12    10    12     words in one caption
title_max_words         8     8     8      words in one title
beat_max_seconds        15    18    15     longest beat
overlap_fraction        0.1   0.1   0.1    text overlap, as a share of the smaller box
plan_min_seconds        30    30    30     longer films need an ask and exactly one aha
frame_slack_seconds     0.05  0.05  0.05   timing slack for frame rounding
```

## 4. Beat template

One storyboard entry per beat, ids in the scene's order (template in SKILL.md).

- Beat k+1's `audience_question` follows from beat k's `takeaway`.
- `changes` is one verb: two verbs, two beats.
- Write the caption from the takeaway, not from the math: why, or what to notice.
- `keep` everything the viewer must still see; never make them remember what you removed [2].
- Read, then watch. Devices and `derive` wait until the last change has been read; call
  `self.pause()` before a plain `play` or `place` that follows a reveal.
- Label things on the picture, in their symbol colors (spatial contiguity, d ≈ 0.79 [1]).
- Leave `hold`, `pause` and `run_time` unset; the viewer's `level` sets them. Only the aha's motion
  gets a `run_time`, 1.5–3 s.

The `picture_to_formula` gallery film, planned and rendered (general):

| Beat | Question entering | The one change | Caption (words) | s |
|---|---|---|---|---|
| hook | Why do these sums land on squares? | 1, 1+3, 1+3+5 with totals | Every total is a square: 1×1, 2×2, 3×3. (8) | 3.9 |
| predict | Will the next sum be a square? | `1+3+5+7 = ?` joins | Add the next odd number, 7. Still a square? (9, `ask`) | 7.1 |
| picture | What does adding 3 look like? | an L of 3 wraps 1 dot | Draw each odd number as an L around the square. (10) | 4.8 |
| again | Does the next one fit too? | an L of 5 wraps 2×2 | The next odd number wraps the square again. (8) | 4.9 |
| why (aha) | Why does the L of 7 fit? | the L of 7 wraps 3×3 in 2 s; `?` → 4² | Two sides of 3, plus a corner: 7 dots. (8) | 8.8 |
| formula | For any number of Ls? | the last sum → 1 + 3 + ⋯ + (2n − 1) = n² | The n-th L has 2n − 1 dots. (8) | 5.9 |
| recap | So why squares? | the L of 7 again, fast; formula boxed | Odd numbers are the Ls of a growing square. (8) | 5.6 |

Its code has no hold, pause or wait; its one `run_time` is the aha's 2 s.

## 5. Seven patterns

Every film uses 1 and 7. Add the one the viewer lacks and start from its template
(`init --template <name>`): keep its beats, swap in your mathematics.

| Pattern | Use when | Template | Kit |
|---|---|---|---|
| 1. Hook by question | Always first: a surprising concrete fact under the question as title; never a definition [15]. | all | `title`, `place` |
| 2. Concrete first | The symbols are new: objects, a labeled picture, then notation, the concrete still in view [4]. | `concrete_first` | `DotArray`, `paint`, `place` a selection |
| 3. Picture to formula | An identity counts or measures something: each term arrives as its part of the picture is marked. | `picture_to_formula` | `DotArray(shown=…)`, `show`, `link`, `derive` |
| 4. Before and after | An operation changes something: change one thing, keep scale and position, say what to compare. | `contrast` | `VectorGrid(fits=…)`, `Readout`, `reserve` |
| 5. Misconception, then repair | `wrong_guess` is plausible: ask for a guess, let it fail on screen, repair it [5, 6]. | `misconception` | `misconception`, `ask`, `Figure` |
| 6. Zoom in on a detail | The key relationship is small (a corner, a term, a limit); keep the whole in view. | `zoom_detail` | `FunctionPlot.inset`, `secant`, `Readout` |
| 7. Recap by replay | Always last: the aha again at about twice the speed, the question answered on screen. | all | the aha as a method; `hide` or `reset` |

## 6. Anti-patterns

| Anti-pattern | Fix | Caught or fixed by |
|---|---|---|
| Narration pasted into captions | ≤ 12 words; labels name things; cut adjectives (coherence, d ≈ 0.86 [1]) | `long_text`, `caption_too_fast` |
| Symbols before meaning | A concrete instance first; the symbol arrives on its object [4] | `unexplained_notation`, `Figure.length` |
| Everything at once | ≤ 3 new things per beat, in order [3] | `crowded_beat`, `too_dense` |
| "Today we'll learn X" | Hook by question; each beat answers the last one's question [7] | `viewer_plan`, `beats.png` |
| Ending on the last algebra line | Replay the aha; the last frame answers the question | `beats.png` final tile |
| Drifting or unlabeled colors | Set them once in `symbols`; label each where it first appears | `palette_overload`, `link` |
| Decorative motion; an equation faded out for the next | Motion that means something [14]; `replaces=` or `derive`, so terms travel | `crowded_moment` |
| Motion while reading, or in two places | Read, then watch; one motion at a time [8] | `short_hold`, `crowded_moment` |
| The brightest thing is not the relevant one | `accent`, `focus` or `highlight` it [9] | `unsignaled_reveal` |
| A legend in a corner | The label beside its object, in its formula color [1] | `annotate(style="label")` |
| "Proof" for a numerical check | "A picture, not a proof"; "for x in radians" | `validate_math` |
| Overlaps and tiny formulas [16] | `place` in regions; obey `CompositionError` | `text_overlap` |

Write captions conversationally, with "you" and questions (personalization, d ≈ 0.79 [1]), and
never read a formula aloud.

## 7. Frames-only critique

Answer each check from frames and captions alone, as a viewer who knows only `knows`. Fix a
failing beat before polishing anything.

1. **Frames only** (`beats.png`). Answer each tile's question from its frame. If your answer
   differs from the takeaway, the beat fails.
2. **First glance** (first tile). Would they know the film's question within 3 s?
3. **Tracking** (`contact_sheet`). The spine never vanishes, jumps or changes color for no reason.
4. **Symbols** (`unexplained_notation`). Each is in `knows` or was shown in an earlier frame.
5. **Colors** (`palette_overload`). One meaning per color, labeled where it first appears.
6. **Load** (`crowded_beat`, `too_dense`). The most salient thing is the relevant one.
7. **Read, then watch** (manual). Nothing important moves while a new caption is being read.
8. **Prediction** (`viewer_plan`, `question_hold_short`). A 3 s pause invites a guess first.
9. **Misconception** (`beats.png`). The `wrong_guess` is on screen, then visibly repaired.
10. **Payoff** (final tile). The question and its answer are both on the last frame.
11. **Honesty** (`validate_math`). Captions keep intuition, evidence and proof apart.
12. **Muted captions** (manual). Without captions, the aha's motion still shows what changed [8].

Re-render only what you fixed; after 2–3 passes that do not converge, report what remains.

## 8. Sources

1. Mayer, *Multimedia Learning* (2021): coherence d .86, signaling .41, segmenting .79,
   pre-training .75, personalization .79; spatial contiguity .79 (2017).
2. Sweller et al. (2011), *Cognitive Load Theory*; Kalyuga et al. (2003), expertise reversal.
3. Cowan (2001), "The magical number 4", *BBS* 24.
4. Fyfe et al. (2014), concreteness fading, *Ed. Psych. Review* 26.
5. Crouch et al. (2004), "Classroom demonstrations", *Am. J. Phys.* 72.
6. Muller et al. (2008), "Saying the wrong thing", *JCAL* 24.
7. Loewenstein (1994), the information gap; Kang et al. (2009), *Psych. Science* 20.
8. Tversky et al. (2002), "Animation: can it facilitate?", *IJHCS* 57.
9. Lowe (2003), salience versus relevance; de Koning et al. (2009), attention cueing.
10. Spanjers et al. (2010), segmenting animations: 2 s pauses helped novices.
11. Rowe (1986), "Wait time": 3 s or more improves answers.
12. Netflix *Timed Text Style Guide* (20 chars/s); BBC *Subtitle Guidelines* (160–180 wpm).
13. Guo et al. (2014), video length and engagement, *L@S*.
14. Rey (2012), the seductive detail effect, *Ed. Research Review* 7.
15. Sanderson (3Blue1Brown), "concrete before abstract"; Newton (1990), tappers.
16. Ku et al. (2025), *TheoremExplainAgent*; Chen et al. (2025), *Code2Video*.
