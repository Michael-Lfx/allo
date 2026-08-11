# Stage Director: scene_plan (cinematic)

Produce `scene_plan` — the shot-by-shot visualization of `script`. Every
scene from `script` must be broken into one or more concrete, filmable
shots.

## Writing good shot descriptions

Each shot's `description` is the seed for its eventual generation prompt
(see `skills/creative/video-gen-prompting.md`), so write it with that in
mind: subject, action, framing, and enough visual specificity that two
different people reading it would picture roughly the same image. Vague
descriptions ("a nice shot of the product") produce inconsistent, generic
generations.

## Motion vs. still

This pipeline's default delivery promise is `motion` — set `is_motion: true`
on essentially every shot unless the human/proposal explicitly called for a
still moment for deliberate emphasis (e.g. a single freeze on a key product
detail). If you find yourself marking many shots as stills "to be safe" or
"to save budget," stop — that silently erodes the locked delivery promise
and will be caught (and blocked) at `compose` anyway; raise the concern via
`decision_log_append` instead and let the human decide.

## Continuity setup

Recurring characters, locations, and props must be described identically
across every shot that features them — this description is what `assets`
will turn into prompts, and inconsistent descriptions produce visibly
inconsistent generations. Consider explicitly noting continuity chains (e.g.
"shot 4 should visually continue from the end of shot 3") so `assets` knows
to use `extract_last_frame` chaining.
