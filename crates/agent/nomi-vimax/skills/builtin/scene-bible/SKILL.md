---
name: scene-bible
display-name: 场景圣经
description: >-
  Lock place function and time of day before characters enter. Use when the
  canvas needs a stable set, not a new location every shot.
category: storyboard
version: "1.0.0"
tags: [canvas, scene]
requirement-overlay: |
  Direct as a scene bible. Lock spatial function, exits, and time of day first.
  Characters enter a known set; do not teleport the room between cuts.
  Canvas: canvas_get_skill → storyboard_inspect / storyboard_apply existing Script rows → spec_inspect → canvas_apply for graph only → canvas_run and wait. Look is a visual slot only. Do not declare done while the queue is busy.
---

# 场景圣经

## When
- 同一空间要贯穿多镜
- 时段、入口、道具位置必须稳定

## 步骤
1. 空间功能（谁在这儿做什么）
2. 时段与主光方向
3. 可复用的空镜 / 建立镜
4. 人物进入已锁场景

## 红线
- 每镜换一个房间
- 用空镜堆气氛而不推进动作
