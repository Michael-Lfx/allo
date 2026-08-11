# Stage Director: publish (hybrid)

Finalize the project once `final_review.pass` is true (or a human has
explicitly overridden a soft fail). Write `publish_log` pointing
`final_video_path` at the same file as `render_report.out_name`, record
any distribution notes the human provided, and confirm the hybrid
delivery promise was kept through to the shipped cut.

Do not invent a new render at publish time. Publish is a bookkeeping and
sign-off stage, not a second compose.
