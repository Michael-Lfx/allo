# Flowy Agent Store — Site（已迁出）

本目录下的站点代码已迁移为**独立仓库**，不再随本 monorepo 维护。

- 本地路径：`C:\workspace\agent-store-site`
- GitHub：`https://github.com/szStarWave/agent-store-site`

站点本体、构建配置、文档与内容均请到上述仓库查看与修改。本文件仅作占位与指向用途。

随站点迁出，与本仓耦合的两道守卫也已退场：`scripts/check-docs-sync.mjs`（中英结构同步 +
`--self-test`）与 `web/packages/client/src/docs-drift.test.ts`（站点文档引用的方法计数）。
它们在站点目录不在本仓时只会说谎（`0 page(s)` 恒绿）或报错（`ENOENT`），因此已删除——**
两者都应在站点仓重建**。另注：站点文档 §7.3 的方法计数仍是 `46 / 65`，应改为 `46 / 68`。
详见 `docs/agent-store/16-sdk-webui-site-priority-plan.zh.md` §8.4。
