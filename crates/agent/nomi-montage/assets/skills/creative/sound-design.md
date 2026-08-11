# Skill: Sound Design (planning only)

**No audio generation or mixing tool exists in this build.** This skill
governs how to *plan for* sound honestly within that constraint — narration
timing, pacing implications, and what to log — not how to produce audio,
because there is currently no tool that does.

## What you can actually do

- Write dialogue/narration with natural spoken rhythm (see
  `skills/creative/storytelling.md`) so that *when* narration audio exists
  (from the Flowy video backend's generated speech, where a model supports
  it, or from a future dedicated tool), timing will already be sound.
- Plan shot `duration_secs` / still `hold_secs` values that give a spoken
  line enough time to complete before the next cut — a common and avoidable
  error is timing a cut to "feel right" visually while cutting off narration
  mid-sentence.
- Note explicit sound-design *intent* in `decision_log_append` (e.g. "beat 3
  should land on a percussive impact — no music tool available, flag for
  human post-production") so a human editor doing final sound design has a
  clear brief instead of having to guess.

## What you must not do

- Do not write into `render_report`, `final_review`, or any artifact that
  music/SFX/mixing was applied when no such tool was called. If the `sound`
  dimension of the seven-dimension rubric is being scored honestly (see
  `skills/meta/reviewer.md`), a piece with no audio pass should score
  accordingly low on that dimension — not be inflated to match the visual
  quality of the rest of the piece.
- Do not claim a specific generated video clip "has" dialogue/music beyond
  whatever the underlying Flowy video model itself actually produced.

## Where this matters most

- **Avatar/talking-head pipelines**: the avatar's own spoken delivery *is*
  the audio — get the script's spoken rhythm right, since there is no
  separate audio pass to fix timing issues later.
- **Hybrid/slideshow pipelines**: still shots have no inherent audio; if the
  piece needs music/ambience under them, that is a human post-production
  step this build cannot perform — say so explicitly rather than silently
  shipping a track-less "hybrid" piece that reads as unfinished.
