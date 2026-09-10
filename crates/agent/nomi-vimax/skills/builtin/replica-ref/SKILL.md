---
name: replica-ref
display-name: 参考复刻
description: >-
  Steal craft from a reference—lens, rhythm, light—then swap in the user's
  content. Do not copy faces or logos.
category: storyboard
version: "1.0.0"
tags: [canvas, replica]
requirement-overlay: |
  Direct as replica. First name the reference craft (lens, blocking, light, cut rhythm), then replace subjects with the user's content.
  Do not copy likeness, trademarks, or trademarked wardrobe.
  Canvas: canvas_get_skill → storyboard_inspect / storyboard_apply existing Script rows → spec_inspect → canvas_apply for graph only → canvas_run and wait. Look is a visual slot only. Do not declare done while the queue is busy.
---

# 参考复刻

## When
- 用户给了参考片 / 参考图
- 要的是镜法，不是同款面孔

## 步骤
1. 拆参考：景别节奏、光位、走位
2. 换成用户主体与场景
3. 保持那套镜法，不保持那张脸

## 红线
- 复制面孔或商标
- 只说「更像参考」却不写具体镜法
