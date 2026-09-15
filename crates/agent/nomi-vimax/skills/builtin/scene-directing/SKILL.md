---
name: scene-directing
display-name: 场面导演
description: >-
  In-frame directing craft for narrative shorts: one scene purpose, shared
  geography before coverage, stimulus-to-leftover performance, living extras
  that do not steal focus, motivated camera, and prop entry/exit states.
  Stacks with 短剧导演. Use for 场面调度, blocking, extras, or when shots feel
  posed, empty, or spatially unreadable.
category: drama
version: "1.0.0"
tags: [drama, directing, blocking, performance, continuity, extras]
director:
  pack-policy: dense
  over-budget: fold
requirement-overlay: |
  Direct the SCENE (blocking, performance, camera, extras) — not the plot engine.
  NORTH STAR: one purpose per scene. Do not restage the same beat as a new story.
  SPACE FIRST: lock shared geography (who stands where, faces whom, what the
  room allows) before camera detail. No readable spatial relation → no coverage.
  PERFORMANCE CHAIN: for each acted or spoken beat write stimulus → first
  visible adjust (eyes / breath / hands) → action or line → leftover state the
  next clip inherits. Goals are "make them …", never mood adjectives.
  IN-FRAME LIFE: every visible person has their own task, attention, and offset
  reaction. Background activity serves the main focus — no idle statues and no
  competing circus.
  CAMERA MOTIVE: angle, move, speed, and light must protect a narrative fact
  the audience can see. Forbidden as a design: photographer names, film titles,
  "cinematic", or a focal length with no visual result.
  PROP STATES: any plot prop in visual_desc has an entry state and an exit
  state; the next row opens on that exit unless the story changes it.
  ASSETS: one reference image, one job (face lock / set volume / prop). Do not
  ask a plate to also be the composition bible.
  OUTPUT: keep this pipeline's storyboard fields (visual_desc, quoted
  audio_desc, CUT TO). Do not emit a six-role review table or a freeform
  prompt dump. Duration and aspect come from the session.
---

# 场面导演 Playbook

## When to apply
- 叙事短片里人物站位、对手戏、群演或道具状态要可拍
- 镜头看起来像摆拍、背景像静物、或空间关系读不出来
- 与「短剧导演」叠用：短剧管钩子/反转，本手册管这一场怎么演

## 场面
1. 先写这一场为什么存在（一个北极星），再写站位
2. 共享空间可读之后才细化机位
3. 刺激 → 微调整 → 动作/台词 → 留给下一镜的出口状态
4. 画面里每个人有自己的事，且不抢主戏
5. 机位和光为必须被看见的事实服务

## 红线
- 用情绪标签或「电影感」代替可演动作和可见结果
- 背景当死布景，或为了热闹让群演盖过主焦点
- 道具有来无去；下一镜对不上上一镜的出口状态
- 输出会诊表、授权门、或另一套提示词模板
