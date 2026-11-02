---
id: x12
title: "枚举集三处同步治理——互钉脚本 + 读宽容 Unknown + 前端全集查表"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# x12: 枚举集三处同步治理

**What to build:** 让「加一个题型/状态」从「DB + 后端 + 前端 N 处手改、漏一处静默误标」变成
「Rust 单源加一个变体，脚本红给你看漏了哪」。三件套（ADR-0015 的执行面）：契约互钉脚本、
落库 enum 读宽容写严格、前端查表全集兜底。

## 现状（盘点 2026-11-01，行号实现会话复核）

- Rust 落库 enum 全部闭合、无 `#[serde(other)]`，`TryFrom<&str>` 对未知值直接 `Err`
  （`models.rs:280-289` CourseGenerationMode、`:445` SectionKind、`:1851` LessonStatus 等 ~15 处）：
  一行新值让旧后端整行反序列化失败，而非降级。
- 前端 `types.ts`（1041 行）手抄 union；`model.ts:11,23` `Record<LessonStatus,string>` 等查表未知
  值返回 `undefined`；`utils.ts:36-53` 未知 state **静默落到 scheduled**；
  `QuestionDialogs.tsx:477-481` 非 single_choice/fill_in_blank **一律标成判断题**（numeric/
  ordering/matching 等 6 种被误标）；`ActivityInput.tsx:146-239` else 分支把未知题型渲染成
  TextArea；自建题创建菜单只开放 3 种（`QuestionDialogs.tsx:365-367`），后端放行 7。
- i18n `learning.json` 各 582 key，按 kind/status 组合 ~101 key，`i18n-keys.d.ts` 由
  `scripts/generate-i18n-types.mjs` 生成；加一个值 = 2 份 JSON + 重生成 + 全部查表。
- 119 处字面量比较散在 21 个文件。仓库已有 `scripts/check-*.mjs` 质量门文化 + script-registry。

## 裁决（2026-11-02 Q12）

**不扩自建题菜单（保持 3 种）**——multi_choice/numeric/ordering/matching 只有 AI 生成路径、
没有人工创建表单，开到 7 等于造 4 个新表单。本票只修 9 种 kind 的标签/渲染全集兜底；人工
创建表单另立产品票（map「波外候选」）。

## 实施要点

- [ ] **互钉脚本** `scripts/check-learning-vocab.mjs`：Rust 侧用一个 `cargo test` 把落库 enum
      变体集导出 JSON 到 `target/`（比正则解析 models.rs 稳），脚本读清单对拼 `types.ts` 对应
      union 字面量、`learning.json` 双语 key 前缀集、x11 的 `phase::ALL`、x17 的 visual 码集、
      残留 DB CHECK 清单（x05 后应为空）；不一致红。登记 script-registry，进 `bun run check`。
- [ ] **读宽容写严格**：每个落库 enum 加 `Unknown(String)` 变体（自定义 Deserialize 保留原文），
      只在**读路径**出现；写路径 `TryFrom` 仍拒；DTO 出站把 `Unknown` 原样透传。
      单测：未知值读通、写拒、透传。
- [ ] **前端全集查表**：`Record<K, V>` 改 `lookup(k): V`（带 default 中性态 + 显示原始码）；
      `questionStateMeta` 显式 unknown；`QuestionDialogs` 题型标签按 9 种全集映射（i18n `kind*`
      ×9 已存在只用了 4 个）；`ActivityInput` 未知 kind 显示「暂不支持的题型：<kind>」而非 TextArea。
- [ ] 不追求一次消灭 119 处字面量；只要求**所有查表有 default、所有 if 链有可见兜底**，脚本
      钉住枚举集一致。

## 验收

- `bun run check` 含新脚本绿；`cargo test -p nomifun-learning`；真机：后端临时注入未知 kind 行
  → 列表/详情可见降级不白屏。

## 非目标

- Rust↔TS 全量 codegen（ts-rs/specta）——远期雾区。
- 自建题人工创建表单（波外产品票）。
