# Stage Director: publish (cinematic)

Write `publish_log` — the final record of where the finished film ended up.
This is the last stage; there is nothing downstream to catch a mistake here.

## What to do

- Confirm `final_review.pass` is `true` before treating publish as routine.
  If it is `false`, this stage's human-approval checkpoint is where a human
  explicitly decides whether to publish anyway (e.g. accepting a known sound
  limitation) or send the project back for more work — do not silently
  publish a failed review.
- Set `publish_log.final_video_path` to the exact same file
  `render_report.out_name` pointed at — do not introduce a discrepancy
  between what was reviewed and what gets recorded as published.
- Use `decision_log_append` to record any human override of a failing
  review, including their stated rationale — this is exactly the kind of
  decision `CONTRACT.md` requires be auditable.

## After this stage

`publish` is the pipeline's terminal stage. Once its artifact validates and
(if required) a human approves, the project's checkpoint status becomes
`succeeded`.
