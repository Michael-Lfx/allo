# Stage Director: review (cinematic)

Apply the seven-dimension rubric (see `skills/meta/reviewer.md` and
`governance::scoring`) to the actual finished cut referenced by
`render_report`, and write `final_review`.

## How to score honestly for a cinematic piece

- **Concept**: does the finished film actually deliver
  `proposal_packet.concept_summary`, or did it drift during production?
- **Script/story**: does the arc from `script` read clearly in the finished
  edit, or did editing choices obscure it?
- **Visual craft**: judge the actual generated shots against the chosen
  style playbook's `visual_language` — not against an abstract ideal.
- **Motion continuity**: for a `motion`-promised piece, this dimension
  should weigh heavily — check for jarring discontinuities between chained
  shots.
- **Sound**: score honestly per `skills/creative/sound-design.md` — if no
  audio pass exists beyond generated dialogue/ambience, this dimension
  should reflect that.
- **Pacing/delivery**: does the cut's rhythm (see `edit_decisions`) actually
  serve the story, or does it feel arbitrary?
- **Technical robustness**: any artifacts, mismatched aspect ratios, or
  compose warnings should lower this score even if everything else is
  strong.

## `pass` and the publish gate

Set `final_review.pass` based on whether `total_score` meets
`governance::scoring::PUBLISH_QUALITY_GATE` (6.0) — do not set `pass: true`
to be agreeable if the honest weighted score is below the gate. A failing
review is a legitimate, expected outcome that sends the project toward
`awaiting_human` at `publish` for an explicit override decision, not a
failure of your job.
