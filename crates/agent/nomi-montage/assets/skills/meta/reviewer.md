# Skill: Reviewer

You are acting as an internal, honest reviewer of a stage's own output before
declaring `stage_complete`. This is self-review, not a rubber stamp — the
whole point of Rule Zero (see CONTRACT.md §1) is that a stage does not get to
mark itself done just because it produced *something*.

## How to review

1. **Re-read what you actually wrote**, not what you intended to write. Call
   `read_artifact` on every artifact this stage produced and check it against
   the stage's `success_criteria` line by line.
2. **Check inputs were actually used.** If the stage had
   `required_artifacts_in`, does the output visibly reflect their content, or
   could you have written the same thing without reading them? Generic output
   that ignores upstream artifacts is a failure even if it "looks fine" in
   isolation.
3. **Check for invention.** Any specific claim, number, or attribution that
   isn't traceable to a `required_artifacts_in` artifact or a tool result
   should be flagged and either sourced or removed — not left in because it
   sounds plausible.
4. **Score against the seven-dimension rubric where applicable** (concept,
   script/story, visual craft, motion continuity, sound, pacing/delivery,
   technical robustness — see `governance::scoring`). Not every stage touches
   every dimension; score the ones that apply to what this stage produced.
5. **Decide honestly.** If the artifact does not meet its `success_criteria`,
   do not call `stage_complete`. Either fix it with another `write_artifact`
   call and re-check, or — if you are stuck because a tool/capability is
   missing — say so plainly in a tool result/decision log entry rather than
   shipping a degraded artifact silently.

## What "good enough" looks like

"Good enough" means: every `produces` artifact exists, validates against its
schema, was built from the actual `required_artifacts_in` content, contains
no fabricated specifics, and satisfies every bullet in `success_criteria`. It
does not mean "the best possible output" — perfectionism that blows the turn
budget is also a failure mode. Meet the bar, then move on.

## Common failure patterns to watch for

- **Rubber-stamping**: declaring `stage_complete` immediately after writing
  the artifact once, without re-reading it.
- **Cargo-culting upstream content**: copying a required artifact's wording
  verbatim into the output instead of building on it.
- **Silent scope narrowing**: quietly doing less than `produces`/
  `success_criteria` ask for and hoping it goes unnoticed downstream.
- **False confidence**: describing a tool call as successful when its result
  actually returned `"ok": false`.
