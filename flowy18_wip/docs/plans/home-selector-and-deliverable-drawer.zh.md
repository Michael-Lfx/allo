# 首页选择器与交付物抽屉 · 实施任务书

状态：2026-11-20 起执行。目标是把已经跑通的首页选择器补完，并加交付物右抽屉，代码全部落盘、测试全绿，交人工实测。

## 已完成基线（均已实测）

1. 20 张 AI 封面已生成并压缩为 384×256 WebP，总计 103KB，位于
   `crates/flowy-studio/assets/starters/`，用 `include_bytes!` 编译期嵌入。
2. 首页已改为「选择器 + 内容区」：居中标语、5 个纯文字 chips、4 张示例卡随 chip 就地切换。
3. 切换动画已接入：180ms opacity 淡入，`ease_out_quint`，只动 opacity 不动布局。
4. 测试基线：`flowy-agent` 38、`flowy-i18n` 5、`flowy-studio` 39，全绿。
5. 未提交改动：`starter/catalog.rs`、`starter/mod.rs`、`studio.rs`、`panel.rs`、`Cargo.toml`、23 份 locale 与 assets。

## 本轮任务

### T1 · chip 选中态过渡

现状：选中/未选中是瞬时换色。

做法：把 chip 的 hover 与选中状态显式保存到视图状态，用 120ms 过渡呈现；GPUI 的 `.hover()` 只提供瞬时样式，不能直接过渡。

验收：切换 chip 时颜色渐变；`cargo check -p flowy-studio` 零警告。

### T2 · 交付物按轮聚合

现状：每次工具调用产生一张文件卡，同一轮写多个文件会刷屏。

做法：把同一轮的连续文件变更折叠成一张汇总卡，显示「N 个文件 · +X −Y」，点开右侧抽屉列出明细。

验收：新增单测覆盖合并与行数累加；一轮内多次写同一路径只留一张卡。

### T3 · 右侧抽屉与文件预览

做法：抽屉从右侧滑出，宽度受限，只显示本次变更的文件列表；点文件可预览文本、Markdown、图片。超过 1MB 或二进制显示「不支持预览」并给出打开所在文件夹。

验收：单测覆盖大小与类型分派；抽屉可开合。

### T4 · 全量验证

命令与预期见下节。三个 crate 必须全绿，并贴出真实输出。

## 允许修改的路径

1. `flowy18_wip/crates/flowy-studio/src/**`
2. `flowy18_wip/crates/flowy-studio/assets/**`
3. `flowy18_wip/crates/flowy-agent/src/projection.rs`（聚合逻辑）
4. `flowy18_wip/crates/flowy-i18n/locales/*.json` 与 `crates/flowy-i18n/src/lib.rs`（新增 key）

不许：改 `flowy-app` / `flowy-desktop`；新增依赖；删除或放松既有测试；提交。

## 验收命令

```sh
cd flowy18_wip
mkdir -p ../.test-tmp
TEMP=D:/workSpace/git_clone_test/allo/.test-tmp \
TMP=D:/workSpace/git_clone_test/allo/.test-tmp \
cargo test -p flowy-agent -p flowy-i18n -p flowy-studio
```

预期：`flowy-agent` ≥38、`flowy-i18n` ≥5、`flowy-studio` ≥39，0 failed。

> 必须带 `TEMP`/`TMP` 重定向：这台机器的系统临时目录写文件被拒（`os error 5`），不带就会有一批与代码无关的失败。

## 防作弊

1. 测试数不得少于 38 / 5 / 39。
2. 不许删测试、跳过测试、放松断言、mock 被测对象。
3. 新功能必须配测试，不许只写实现。
4. 报告里贴真实命令输出，不写「应该能过」。

## 替领导拍的板

1. 右抽屉只收文件变更，不收工具调用详情。
2. 预览只支持文本、Markdown、图片；PDF 和 Office 不做，范围太大。
3. 动画只做 120ms 状态过渡与 180ms 内容切换，不做入场编排。
4. 不做提交，等人工实测后再定。

## 断点与止损

1. 进度写 `flowy18_wip/PROGRESS.md`，接手先读它。
2. 阻塞写 `flowy18_wip/BLOCKED.md`。
3. 同一验收连败 3 次换方案；结果比基线差就回滚并如实报告。
