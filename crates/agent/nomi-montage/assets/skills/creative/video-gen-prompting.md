# Skill: Video/Image Generation Prompting

Applies to `assets` (and `avatar_render`/`talking_head_render`-equivalent)
stages: turning a `scene_plan` shot description into a prompt for
`flowy_image` / `flowy_video` (directly, or via `image_selector` /
`video_selector`).

## Structure a prompt around what the camera sees

A good shot prompt reads like a shot list entry, not a mood board: subject,
action, framing/angle, lighting, and — when a style playbook applies —
its `prompt_modifiers` appended as a clause (see
`crate::styles::StylePlaybook::prompt_clause`). Avoid vague adjectives
("beautiful", "amazing") that don't constrain the output; prefer concrete
visual specifics ("low-angle, backlit by a single window, dust visible in
the light").

## Consistency across a sequence

- Repeat identifying details of recurring characters/props/locations
  verbatim across every shot that features them — the generator has no
  memory between calls, so consistency is entirely your responsibility as
  the prompt author.
- For chained motion continuity, use `extract_last_frame` on the previous
  clip and pass it as `first_frame` to the next `flowy_video` call rather
  than re-describing the pose/framing in text and hoping it matches.
- Apply the project's style playbook's `prompt_modifiers` to every single
  shot, not just the ones that feel like they need it — inconsistent style
  application across shots is one of the fastest ways a sequence reads as
  disjointed.

## Motion vs. still shots

- For a motion shot, describe the action across the clip's duration
  ("she turns from the window and walks toward camera"), not just a static
  pose — a video prompt describing a static pose tends to produce a
  near-static clip, which quietly erodes the delivery promise.
- For a still shot, compose it to reward a longer look: give it a clear
  focal point and enough visual detail to justify its `hold_secs`.

## Handling generation failures

If `flowy_image`/`flowy_video` (or a selector) fails, don't retry the exact
same prompt hoping for a different result — content-safety rejections and
composition failures usually need a genuinely different prompt (soften
sensitive wording, simplify an overly complex composition) to succeed. If
several honest attempts still fail, say so in `decision_log_append` rather
than silently substituting an unrelated placeholder image.
