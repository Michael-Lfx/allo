# Stage Director: compose (framework-smoke)

Write a placeholder-but-schema-valid `render_report` whose fields are
filled (runtime/out_name) without claiming a real media render occurred.
Append to `decision_log` that no real media was generated in smoke mode.
Do not invoke video_compose against missing files.
