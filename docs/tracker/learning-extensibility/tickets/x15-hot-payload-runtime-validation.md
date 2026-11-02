---
id: x15
title: "热点 payload 运行时宽容校验——旧客户端遇新后端降级而非白屏"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# x15: 热点 payload 运行时宽容校验

**What to build:** 在 `learningApi` 边界对 5–8 个热点响应/WS 帧做**宽容** schema 校验（未知
字段透传、未知枚举值映射 unknown），替代 `json.data as T` 的零校验。

## 现状（盘点 2026-11-01，行号实现会话复核）

- `ui/src/common/adapter/httpBridge.ts:580 httpRequest<T>`，`:748/:828` `json.data as T` 零运行时
  校验；learning 目录无 zod/valibot；`pages/learning/api.ts:49 learningApi`。
- Rust `models.rs`（3869 行）↔ TS `types.ts`（1041 行）无生成链路，全手抄。
- 后端枚举增值 → 前端「成功」接收 → 查表 `undefined` / if 链误标（x12 修兜底，本票修入口）。

## 实施要点

- [ ] map 裁决点 4：先查 `ui/package.json` 是否已有 zod/valibot/superstruct；无则选型并走
      CONTRIBUTING 许可核查；也可零依赖手写 `guard<T>()`（热点少，可接受）。
- [ ] 热点清单：课程列表项、课程详情、课时详情（sections/activities）、复习 due/answer、
      structure view、WS course/lesson 两帧。每个 schema `passthrough()` + 枚举
      `.catch('unknown')` + 可选字段默认。
- [ ] 失败策略：schema 失败**不抛**——console.warn + 返回 best-effort 对象；只有顶层形状完全
      不对才按网络错处理。
- [ ] 与 x12 配合：unknown 枚举值落到前端 lookup default。
- [ ] 测试：每个 schema 一组「旧形/新形/多字段/未知枚举」样例。

## 验收

- `bun run typecheck`；前端单测绿；真机：后端临时返回多字段/新枚举值，页面可见降级。

## 非目标

- 全量 DTO codegen；非 learning 域。
