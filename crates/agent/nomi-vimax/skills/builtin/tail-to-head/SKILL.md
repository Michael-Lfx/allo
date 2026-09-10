---
name: tail-to-head
display-name: 尾接首
description: >-
  Force continuity: the last frame of shot n must read as the first frame of
  shot n+1. Use for walk-ins, handoffs, and match cuts.
category: storyboard
version: "1.0.0"
tags: [canvas, continuity]
requirement-overlay: |
  Direct as tail-to-head continuity. Adjacent shots must match exit pose, eyeline, and screen direction.
  Write the outgoing frame of n as the incoming frame of n+1.
  Canvas: canvas_get_skill → storyboard_inspect / storyboard_apply existing Script rows → spec_inspect → canvas_apply for graph only → canvas_run and wait. Look is a visual slot only. Do not declare done while the queue is busy.
---

# 尾接首

## When
- 走位进出、递接道具、动作匹配剪
- 相邻镜不能跳切丢连续性

## 步骤
1. 写清每镜结束姿态
2. 下一镜第一帧承接该姿态
3. 轴线与左右屏向保持一致

## 红线
- 尾帧与下镜首帧对不上
- 用硬切掩盖走位错误
