# Stage Director: video_analysis (screen-demo)

Produce `video_analysis_brief` that catalogs what actually happens in the
uploaded recording.

## Priorities
- `source_video_path` must point at the real uploaded file.
- `detected_scenes` must cover from 0 to end without large unexplained gaps.
- Describe UI states and user actions concretely (what changed on screen),
  not vibes.

## Rules
- Prefer observable facts over inferred intent.
- Note segments that are too noisy/redundant to keep in the final demo.
- This stage replaces market research — stay inside the recording.
