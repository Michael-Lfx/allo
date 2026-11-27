# ADR-0018: Personal Agent 自动化：本机两表数据集与共享服务

Status: Accepted

Date: 2026-11-27

## Context

`flowy18_wip` 是单用户本机桌面应用。自动化需要做三件事：模型在对话里通过工具
创建和管理自动化，用户在自动化页面做同样的管理，以及到点或手动触发后台运行。

早期方案按企业级多租户设计，包含账户隔离、grant 账本与审批卡片、命令收据与
幂等日志、投递发件箱、全量审计与 schema 契约、第二套工作区锁和运行预算。单机
单用户场景用不到这些，反而让一个功能改动了 168 个文件。本决议以最小实现替代。

## Decision

1. **独立本机数据集**：`<store_root>/automation/automations.sqlite3`，只有
   `automations` 和 `runs` 两张表。ID 为 UUIDv7（`AutomationId` / `RunId`），
   表间引用由应用维护，不建物理外键。版本用 `PRAGMA user_version`，迁移只向前；
   打开时遇到无法识别的版本，把旧文件改名为 `.bak-<ts>` 后新建空库。
2. **一个服务入口**：`AutomationService` 同时服务自动化页面和模型工具
   （`automation_create/list/get/update/pause/resume/delete/run_now/history/cancel_run`），
   两边共用 `AutomationInput`，没有第二套命令层。
3. **一个后台 worker**：每 30 秒 tick 一次，保存或手动运行时立即唤醒。先执行
   Queued 的 run，再处理到期且启用的自动化。错过多次只补跑一次。单个 run 总
   超时 10 分钟。
4. **登录门槛**：未登录时 worker 暂停；登出时暂停并取消进行中的 run。不做账户
   隔离，同一台机器上的数据属于当前用户。
5. **权限只分只读和读写**：自动化会话的工作区锁跟随它的权限，沿用现有
   `WorkspaceLockTable`，不另起一套锁；锁冲突时 run 记为 `Skipped`。模型创建
   的自动化默认只读，只有当前对话本身是读写模式时才允许读写，否则降级并在
   结果里说明。
6. **不递归**：自动化运行产生的会话不注册自动化管理工具。
7. **崩溃语义**：启动时把仍为 `running` 的 run 标记为 `interrupted`，不自动
   重试，用户需要时手动“立即运行”；每个自动化保留最近 20 条已结束的 run。
8. **结果可见**：每次运行是一个用户可以打开的普通会话；结束时发系统通知，通知
   深链到对应 run。

## Consequences

- 自动化相关代码集中在 `flowy-automation`（model / schedule / store / service /
  worker / executor）、`flowy-agent` 的工具与执行器，以及 `flowy-app` 的一个
  接线模块，不再改动 agent 内核。
- 运行使用当前默认模型和默认工具集，不支持按自动化单独设置模型、预算或工具。
- 不支持多账户共享同一数据目录。将来真要支持，需要另开 ADR，在两张表上加
  owner 列并配上迁移。
- 投递重试、结果保留标记、审批流程都不提供；需要重跑时用“立即运行”。
