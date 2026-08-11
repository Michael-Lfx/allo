# Stage Director: publish (talking-head)

Write `publish_log` only after `final_review.pass` is true or a human has
explicitly overridden a soft fail. Point `final_video_path` at
`render_report.out_name`. Do not re-render. Record destination/channel
notes if the human supplied them.
