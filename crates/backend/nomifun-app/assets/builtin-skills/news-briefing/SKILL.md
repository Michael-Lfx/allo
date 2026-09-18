---
name: news-briefing
description: "Use when the user wants a morning briefing, news recap, or spoken news video. Never invent today's news with web_search. Call briefing_create / briefing_status instead."
---

# News briefing

You do **not** write a news script from model memory or `web_search`.

1. Call `briefing_create` with the user's `intent`. Do not require source URLs — the engine researches independent domains from the intent.
2. Optional extras when the user already has them: `source_urls`, `format_secs` (30–300, default 90), `time_window_hours` (default 24), `research_depth` (`fast` or `deep`).
3. Poll `briefing_status` until `succeeded`, `hold`, `failed`, or `cancelled`.
4. On success, point the user to `/video-generation/briefing/{briefing_id}`.
5. On `hold`, the engine still refused to invent facts (research did not yield two independent domains). Suggest refining the topic or pasting extra URLs — do not write the news yourself.

Narration uses the session or install-wide TTS setting. If synthesis is not configured, the run fails with `tts_unavailable` instead of exporting a silent video.

Never paste a homemade "today's news" monologue into chat.
