---
name: nomifun-skills
description: 'Discover and install community AI agent skills. Use when the user asks whether a skill exists, wants marketplace skills (ClawHub / SkillHub), or needs to install a skill. This skill is fully bundled — do not curl a remote guide first.'
version: 1.1.0
---

# Nomi Skills Guide (bundled)

This file is the **baseline** guide shipped with Flowy. You already have it.
Do **not** treat fetching a remote `SKILL.md` as a prerequisite.

## Critical: how this Skill tool works

Invoking `Skill` with `skill: "nomifun-skills"` only returns this guide.

- There are **no** subcommands such as `list`, `search`, or `download`.
- Do **not** pass fake market queries via `args` (for example `args: "search pdf markdown"`).
  Those strings are not executed; they are at best appended as plain text.
- If the user asks whether a skill exists, answer from:
  1. The skills already listed in the system prompt / session, and
  2. The Flowy Skill Market workflow below.
  Then stop. Do not re-invoke this skill in a loop.

## Built-in Skill Market (preferred)

Flowy already syncs public rankings from:

| Source | Site |
| --- | --- |
| ClawHub | https://clawhub.ai/ |
| SkillHub | https://www.skills.sh/ |

**For users:** Settings → Skills → Skill Market. Sync rankings, search/filter, then use **Add** to open an install draft conversation with a reviewed install command.

**For you (agent) when asked to find/install a community skill:**

1. Prefer skills already available in this session. If one fits, say so and use it.
2. If the user needs a marketplace skill, tell them to open **Settings → Skills → Skill Market**, or follow an install draft they already started from that UI.
3. Before running any install command, verify the source page and command, then ask the user to confirm.
4. Typical install command shapes (examples only — prefer the command from Skill Market):
   - ClawHub: `openclaw skills install @owner/slug`
   - SkillHub: `npx skills add owner/slug` (exact form depends on the listing)

Installing a skill is a shell/network action. This guide itself does not download packages.

## Optional guide refresh (not required)

A newer marketplace guide may be published later. Refresh is **optional**.

```bash
mkdir -p ~/.config/nomifun-skills
curl -fsSL https://skills.nomifun.com/SKILL.md -o ~/.config/nomifun-skills/SKILL.md.tmp
```

Only keep the download when it looks like a real skill document:

- starts with YAML frontmatter (`---`)
- contains a `name:` field
- is Markdown text, not an HTML parking / redirect page

```bash
# keep only if the download looks valid; otherwise discard and keep this bundled guide
head -n 5 ~/.config/nomifun-skills/SKILL.md.tmp
```

If the remote is down, returns HTML, or fails validation, **keep using this bundled guide**. Never block the user's task on a remote fetch.

Bundled baseline version: `1.1.0`.

## When to use this skill

- User asks whether a reusable skill exists for a task
- User wants community / marketplace skills
- User asks how to discover or install skills in Flowy

## When not to use this skill

- You can complete the task with tools you already have
- The needed skill is already loaded in this session — just use it
- Do not use this skill as a search API
