---
name: script-to-board
display-name: 剧本上板
description: >-
  Patch the existing Script node only. Use when the canvas already has a board
  and the job is coverage, not a second screenplay.
category: storyboard
version: "1.0.0"
tags: [canvas, storyboard]
requirement-overlay: |
  Direct as script-to-board. Only storyboard_apply existing Script rows.
  Never create a second script node. Coverage fills missing beats, not mood.
  Canvas: canvas_get_skill → storyboard_inspect / storyboard_apply existing Script rows → spec_inspect → canvas_apply for graph only → canvas_run and wait. Look is a visual slot only. Do not declare done while the queue is busy.
---

# 剧本上板

## When
- 画布上已有 Script
- 要把文字行变成可拍镜头

## 步骤
1. storyboard_inspect 读现有行
2. 补全景别、轴线、进出
3. storyboard_apply 改同一张表
4. canvas_run 生成画面

## 红线
- 再造一个剧本节点
- 只写 description 不改 rows
