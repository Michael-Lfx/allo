# Stage Director: edit (hybrid)

Produce `edit_decisions`. See `skills/creative/video-editing.md` for general
craft.

## Hybrid-specific tuning

- Set a deliberate `hold_secs` for every still shot based on how much visual
  or narrative information it carries and what the narration timing over it
  requires — never leave every still at the same default duration.
- Distribute stills across the timeline; if `scene_plan` left two stills
  adjacent, consider whether reordering (moving a motion shot from elsewhere
  in the cut between them) improves rhythm without breaking narrative logic.
- Before finalizing, mentally walk the shot list end to end and count the
  longest run of consecutive stills — if it's 3 or more, restructure before
  calling `video_compose`, since `slideshow_risk` will likely block it
  anyway and it's cheaper to fix here than after a failed compose attempt.
