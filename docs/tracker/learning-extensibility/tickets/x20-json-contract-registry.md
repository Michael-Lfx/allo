---
id: x20
title: "JSON 契约总表进 id_schema_contract——持久化表面一处总账"
type: task
status: open
assignee:
blocked_by: [x09]
part_of: ../map.md
---

# x20: JSON 契约总表进 id_schema_contract

**What to build:** 在既有运行时契约注册表里登记每个 JSON 文档列的 `{列, 信封类型, 当前
version, 可再生?, 演进规则}` 与应用层不变量，配测试断言「写侧版本常量 == 登记版本」
「每个 JSON 列都有登记」。

## 现状（盘点 2026-11-01，行号实现会话复核）

- `nomifun-db/src/id_schema_contract.rs` 已登记：产品表（:67-93）、UUIDv7 业务列（:176-192）、
  非引用 id 列（:292-321）、逻辑引用+删除策略（:892-962）、JSON 逻辑引用（:1082/:1091/:1153）、
  守卫触发器白名单（:1599）；`tests/id_schema_contract.rs` 全表契约测试。
- JSON 列版本契约散在：`docs/contracts/learning-structure-artifacts.md:196-215`（structure
  信封）、`docs/contracts/learning-course-source-plan.md`、各结构体注释——**无总表**；
  `data-and-storage.md:162-186` 迁移计数过时。
- 应用层不变量登记：`source_kb_id`↔`source_kb_ids[0]`、`source_path`↔`source_paths_json[0]`
  双写是 **ADR-0011 已接受的刻意决定**（单列是兼容位，登记 enforced_by=ADR-0011 + 写点成对
  维护）；`concept_registry.canonical` 全库唯一仅代码保证（070）；findings 台账 kind/code 缺列
  （x06 记档）；`generation_mode` CHECK 含代码已不认的 `'full'`（x02 记档）；已发布 `version`
  列实为修订计数（CONTEXT.md「Generation」legacy 口径，不改名）。

## 实施要点

- [ ] 注册表新节 `JSON_DOCUMENT_COLUMNS: &[(table, column, envelope, version, regenerable,
      notes)]`；版本常量从各 crate 引用——nomifun-db 不能依赖 learning crate → 学习 crate 的
      测试读 nomifun-db 登记表与自身常量反向比对。
- [ ] 应用层不变量节 `APP_INVARIANTS: &[(scope, statement, enforced_by)]`——至少把上面五条
      落进去；每条给 enforced_by 指向校验函数或「未执法」。
- [ ] 测试：schema 扫描所有 `*_json` 列必须在登记表（x09 认定的「纯数组不包」记
      envelope=array）；版本比对。
- [ ] 文档：`data-and-storage.md` 新节「JSON 文档列契约」指向登记表为权威；迁移计数改为
      「见 migrations 目录」。

## 验收

- `cargo test -p nomifun-db -p nomifun-learning` 全绿；新增 `*_json` 列不登记即红。

## 非目标

- 修任何不变量本身（只登记）。
