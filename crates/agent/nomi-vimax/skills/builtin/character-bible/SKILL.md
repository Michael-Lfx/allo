---
name: character-bible
display-name: 角色圣经
description: >-
  Lock character assets before boarding. Place this playbook on a canvas,
  generate sheets and turnarounds, then write shots that reuse those assets.
category: storyboard
version: "1.0.0"
tags: [canvas, character]
requirement-overlay: |
  Direct as a character bible. Identity is locked before any coverage.
  Never change face, wardrobe, age, or body type mid-sequence.
  Canvas: canvas_get_skill → storyboard_inspect / storyboard_apply existing Script rows → spec_inspect → canvas_apply for graph only → canvas_run and wait. Look is a visual slot only. Do not declare done while the queue is busy.
---

# 角色圣经

## When
- 新角色还没有设定图 / 三视图
- 多镜必须同一张脸

## 步骤
1. 全身设定图（服装、禁变项写进提示）
2. 三视图
3. 表情与手势
4. 再写分镜，引用这些资产

## 红线
- 中途换脸换衣
- 另起剧本节点替代现有 Script
