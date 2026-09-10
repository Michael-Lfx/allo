---
name: multi-shot-cut
display-name: 动态多切
description: >-
  Cover the beat with montage, not one long take. Use when energy, impact, or
  geography needs cuts.
category: storyboard
version: "1.0.0"
tags: [canvas, coverage]
requirement-overlay: |
  Direct as multi-shot coverage. Default to montage: insert, reverse, insert-detail.
  Do not collapse a scene into one long take unless the user asked for it.
  Canvas: canvas_get_skill → storyboard_inspect / storyboard_apply existing Script rows → spec_inspect → canvas_apply for graph only → canvas_run and wait. Look is a visual slot only. Do not declare done while the queue is busy.
---

# 动态多切

## When
- 动作、反应、空间关系需要切开
- 一镜到底会丢掉冲击或地理

## 步骤
1. 主镜交代关系
2. 反应 / 细节插入
3. 回切推进
4. 剪辑点写进分镜行

## 红线
- 默认一镜到底
- 切了但轴线乱、空间不可读
