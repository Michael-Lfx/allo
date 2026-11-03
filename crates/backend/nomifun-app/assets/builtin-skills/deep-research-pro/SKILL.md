---
name: deep-research-pro
description: "Multi-source deep research. Use when the user wants a cited report, a deep dive, or a decision brief from public web sources. Search with web_search, read key pages with web_extract, and deliver a sourced report. Do not use for a single quick fact that one search already answers."
---

# Deep Research Pro

Produce a thorough, cited report from public web sources. This skill ships with the client. Do not install another copy, and do not look for a Clawdbot or DuckDuckGo helper script.

## When to use it

Use it when the user asks to research, compare, or brief a topic that needs several independent sources. Skip it when one `web_search` snippet already answers a narrow question.

## Workflow

### 1. Understand the goal

If the goal is ambiguous, ask one short clarifying question: learning, a decision, or a written deliverable. If the user says to just research it, continue with a reasonable default.

### 2. Plan before searching

Break the topic into 3–5 sub-questions. For "impact of AI on healthcare":

- main applications in use today
- measured clinical outcomes
- regulatory constraints
- leading organizations
- market size and trajectory

### 3. Search one sub-question at a time

For each sub-question, call `web_search` once with a focused query. Use `count` 5 unless the user asked for broader coverage, and never above 10. Read those results before searching again. Run another query only when the first pass is thin or contradicts itself. Prefer official, academic, and established news sources over blogs and forums. Prefer sources from the last 12 months when recency matters.

Do not fire equivalent searches in parallel.

### 4. Read the pages that matter

When snippets are not enough, call `web_extract` with the 1–3 most relevant public URLs. Do not extract every hit. If more pages are necessary, extract the next small batch only after using the pages already in hand.

Do not write a shell or Python script to download and strip HTML. `web_extract` is the reader.

### 5. Write the report

```markdown
# [Topic]: Deep Research Report
*Generated: [date] | Sources: [N] | Confidence: [High/Medium/Low]*

## Executive Summary
[3–5 sentences]

## 1. [Theme]
- Finding ([Source name](url))

## Key Takeaways
- ...

## Sources
1. [Title](url) — one-line summary

## Methodology
Sub-questions, queries run, and pages read.
```

### 6. Deliver

- Short topics: put the full report in the reply.
- Long reports: reply with the executive summary and key takeaways, and write the full report to `research/<slug>/report.md` in the conversation workspace.

## Quality rules

1. Every claim needs a source. No unsourced assertions.
2. If only one source says it, mark it unverified.
3. Prefer recent sources when the topic moves quickly.
4. Say when a sub-question has insufficient public evidence.
5. Do not invent facts, numbers, or citations.

## Examples

- "Research the current state of nuclear fusion energy"
- "Deep dive into Rust vs Go for backend services"
- "Research strategies for bootstrapping a SaaS business"
