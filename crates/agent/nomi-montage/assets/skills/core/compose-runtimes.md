# Skill: Compose Runtimes

`edit_decisions.render_runtime` names the compositing engine that should turn
a shot list into a finished cut. This build ships exactly one working
implementation; the others are declared in the schema for forward
compatibility and to make the gap between "planned" and "implemented"
explicit rather than hidden.

| Runtime      | Status this build            | What it's for                                                   |
|--------------|-------------------------------|-------------------------------------------------------------------|
| `ffmpeg`     | **Implemented** (default)     | Linear concatenation of generated shots — see `skills/core/ffmpeg.md`. |
| `remotion`   | Declared, not implemented     | Declarative, code-driven overlays/captions/data graphics.        |
| `hyperframes`| Declared, not implemented     | Frame-accurate multi-layer compositing (e.g. paper-craft parallax). |

## The rule

Calling `video_compose` with `render_runtime` set to anything other than
`ffmpeg` returns an explicit `tool not available` error — it does **not**
silently fall back to ffmpeg. This is deliberate: a Remotion composition
implies visual elements (animated captions, data-driven overlays) that a
plain ffmpeg concatenation cannot produce, so falling back silently would
quietly ship a different, lesser deliverable than what was planned. That is
exactly the "no silent downgrade" failure mode CONTRACT.md exists to prevent.

## What this means for you as a stage director

If your creative plan genuinely calls for `remotion` or `hyperframes`
capabilities (see `skills/meta/animation-runtime-selector.md` for when that
is), set `render_runtime` to that value in `edit_decisions` honestly, then
expect `video_compose` to fail with a clear "not available in this build"
message when the `compose` stage runs. That failure is the mechanism working
correctly — it surfaces the gap to the human via the `compose` stage's
`awaiting_human` checkpoint rather than after a real render already spent
budget on the wrong format. The human can then choose to accept an
ffmpeg-only cut (re-plan `edit_decisions` with `render_runtime: ffmpeg`) or
hold the project until that runtime ships.

Never write `render_runtime: ffmpeg` while designing the piece as if a
non-ffmpeg-only capability (e.g. animated on-screen captions) will be
present in the output — that mismatch between what you planned and what you
declared is itself a form of silent downgrade.
