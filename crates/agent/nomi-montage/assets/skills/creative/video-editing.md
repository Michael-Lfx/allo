# Skill: Video Editing

Applies to `edit` (and `capture_plan`/`edit`-equivalent) stages: turning an
approved shot list into a concrete, orderable timeline.

## Pacing

- Cut on meaning, not on a fixed rhythm: a beat ends when its idea or emotion
  has landed, not after a uniform N seconds.
- Vary shot length deliberately. A sequence of identically-timed cuts reads
  as mechanical regardless of content quality.
- Reserve your longest holds for the shot doing the most emotional or
  informational work; reserve your shortest cuts for connective-tissue shots
  that exist to maintain momentum between two more important beats.

## Continuity

- Keep a consistent sense of screen direction and spatial logic across
  adjacent shots of the same scene — don't whipsaw the viewer's spatial
  model of a location without a establishing shot to reset it.
- When `extract_last_frame` was used to chain motion clips, verify the edit
  order actually preserves that visual continuity (the chained clip should
  immediately follow the one it was extracted from).

## Working with mixed motion/still (hybrid delivery_promise)

- A still shot earns its place by composition and information density, not
  by being cheaper than a video clip. If a still doesn't hold interest for
  its planned `hold_secs`, either shorten the hold or reconsider whether
  that beat should be motion instead.
- Never let three or more stills run back to back — `slideshow_risk`
  penalizes exactly this pattern (see `governance::slideshow_risk`), and more
  importantly it actually reads worse to a viewer regardless of the score.

## Common mistakes

- Treating `edit_decisions` as a copy of `scene_plan`'s shot order by
  default. Re-evaluate order deliberately — sometimes the best cut
  reorders shots relative to how they were planned.
- Padding hold durations to fill a target runtime instead of trimming the
  script/shot list to the length the material actually supports.
