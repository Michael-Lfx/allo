---
id: s3
title: "提名装配 expect(\"just pushed\") → Err 化"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# s3: 提名装配零 panic 红线（m1 同族）

**What to build:** `service/learning_graph.rs` 提名候选集装配里
`find(...).expect("just pushed")` 换成 Err 化兜底——`push_nomination` 对
空名/重名**静默早退**，唯一炸点是 `new_aliases[].name` trim 后为空白时
find 落空 panic。上游 parse 已挡空名（目前不可达），但按模块自立的
「图模块生产代码零 panic」红线补齐兜底，与 m1 的 apply 面修复同款。

## 法证证据（2026-11-11 审计，实码复核）

- `push_nomination`（learning_graph.rs:3428-3443）：`name.trim()` 为空或
  批内重名时**不 push 且不报错**。
- `:3459-3462`：`push_nomination(...)` 后立刻
  `find(|plan| plan.name == nomination.name.trim()).expect("just pushed")`。
  重名场景 find 仍命中先前 push；唯一炸点 = 空白名。
- 上游 `growth_contract.rs:374-386`：new_aliases 三字段 trim 后非空
  红条（`ng.aliases.required`）——绕过 parse 的新调用面无兜底。
- 模块红线出处：`name_adjudication.rs:156-158` 注释明写图模块生产代码
  零 panic；m1 修复先例：`structure_ops.rs:640-647` expect →
  `ok_or_else(...)?`，配套红绿样测试 `:1075-1130`。

## 实施要点

- [ ] `expect` 换 let-else / match：find 落空 → 往 `errors` 推 GateError
      （kind 沿 `ng.aliases.*` 命名族，如 `ng.aliases.empty_name`，message
      与 `no_neighbor` 同款散文措辞），`continue` 下一条提名。
- [ ] 红绿样测试：绕过 parse 直灌空名 alias → 不 panic 且 errors 含该
      kind；正常 alias 路径行为不变（既有测试兜住）。
- [ ] `cargo test -p nomifun-learning learning_graph` 过。

## 验收

- 非 test 代码 `expect` 归零于该函数；新测试红绿两面齐。
- nomifun-learning 全套无回归（并入收图验证）。

## 非目标

- parse 层措辞与红线（growth_contract.rs 不动）。
- push_nomination 的静默早退语义本身（重名早退是既有行为）。
