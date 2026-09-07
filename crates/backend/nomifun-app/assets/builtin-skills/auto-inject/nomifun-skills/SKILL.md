---
name: nomifun-skills
description: 'Discover and install community AI agent skills through Flowy Skill Market. Use when the user asks whether a skill exists, wants a SkillHub marketplace skill, or needs to install a skill. This skill is fully bundled and does not require a remote guide or external installer.'
version: 1.1.0
---

# Nomi Skills Guide (bundled)

This file is the **baseline** guide shipped with Flowy. You already have it.
Do **not** fetch a remote `SKILL.md` or invoke an external installer as part of this workflow.

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

Flowy already syncs public SkillHub rankings from:

| Source | Site |
| --- | --- |
| SkillHub | https://skillhub.cn/ |

**For users:** Settings → Skills → Skill Market. Sync rankings, search/filter, then use **Install**. Flowy downloads, validates, and installs the SkillHub archive into its managed Skill directory.

**For you (agent) when asked to find/install a community skill:**

1. Prefer skills already available in this session. If one fits, say so and use it.
2. If the user needs a marketplace skill, tell them to open **Settings → Skills → Skill Market**.
3. For a SkillHub item, use the market card's **Install** action. The Flowy backend performs the network download and validation; do not invoke a shell, `npx`, Node, uv, or OpenClaw command for this flow.
4. If the item is not shown or installation fails, report the market error and ask the user to refresh the market or resolve the named Skill conflict. Do not invent an alternative install command.

Installing a Skill through Skill Market is a Flowy-managed network action. This guide itself does not download packages.

Bundled baseline version: `1.1.0`. If this guide changes, Flowy updates it with the application; do not fetch or overwrite it from a conversation.

## When to use this skill

- User asks whether a reusable skill exists for a task
- User wants community / marketplace skills
- User asks how to discover or install skills in Flowy

## When not to use this skill

- You can complete the task with tools you already have
- The needed skill is already loaded in this session — just use it
- Do not use this skill as a search API
