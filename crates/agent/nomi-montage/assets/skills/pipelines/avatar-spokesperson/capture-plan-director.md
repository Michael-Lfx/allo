# Stage Director: capture_plan (avatar-spokesperson)

Produce `scene_plan` as a capture plan: exactly one segment (shot) per
script scene unless the human explicitly requested coverage variants.

## Priorities
- Map segment durations to speakable dialogue length (do not schedule
  8 seconds of talk into a 3-second clip).
- Note framing (medium close-up is the default spokesperson frame) and
  any cutaway/b-roll insert points as separate non-avatar shots if needed.
- Declare every avatar performance shot as `is_motion: true`.

## Before stage_complete
Confirm a 1:1 scene→segment mapping and that each segment description
includes persona reminders the render stage must restate in prompts.
