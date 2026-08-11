# Stage Director: scene_plan (hybrid)

Produce `scene_plan`, assigning `is_motion` deliberately per shot.

## Placement rules of thumb

- Use a still where a single composed image carries more weight than
  movement would (a key object, a face in a decisive moment, an
  establishing/context shot).
- Use motion where change, action, or spatial movement is the point of the
  beat.
- Never place three or more consecutive stills — break up any run of
  quiet/reflective beats with at least one motion shot, both because
  `slideshow_risk` penalizes long still runs and because it reads better.
- Keep the overall still fraction comfortably within
  `hybrid_motion_still`'s allowed range (no more than ~55-60% stills, even
  though the hard floor is 40% motion) so a shot or two failing to generate
  as motion at `assets` doesn't push the project into a governance block.

## Description quality

Still shots especially need strong, specific descriptions — a still has to
justify its `hold_secs` on composition alone, with no motion to sustain
interest. See `skills/creative/video-gen-prompting.md`.
