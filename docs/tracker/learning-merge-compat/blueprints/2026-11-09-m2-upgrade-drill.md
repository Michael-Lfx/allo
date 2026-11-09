# Blueprint: m2 真机升级演练——001–065 老库过 070（2026-11-09）

<!-- 票：docs/tracker/learning-merge-compat/tickets/m2-orphan-subtree-sweep-migration-070.md
     真机判据的执行方案。双二进制（main 时代出数据 → learn0928 尖端升级），
     全程隔离数据目录，不碰本机真实 dev 库。 -->

## 目标与判据（票面验收逐字）

001–065 老库（**含一门删课保留复习的课程**）→ 应用 070 →

1. 迁移成功启动，**不触发 retired-datasets 退役**（001–065 是 001–068+070 的
   真前缀）；
2. `checkin today.due_count` **不含幻影卡**（删课保留的卡不再计数）；
3. 日历到期桶 / 记忆预测（`/stats/memory`）**归真**（零死课程条目）；
4. 问题管理列表**无幽灵行**；
5. **活课程出卡正常**（due 队列、课时打开、作答全流程 200）；
6. 保全面：活课程行数+内容字节守恒、自建题/其复习史不动、
   `learning_review_events`/`learning_checkins` 行数不变、旧图课程全清。

## 阵地与隔离（红线）

| 项 | 值 |
| --- | --- |
| 工作根 | `C:/Users/test/Desktop/work/mc-drill/`（数据/证据/脚本，仓库外） |
| 老代码 worktree | `git worktree add ../allo-drill-main 8fd9a4fb0`（=origin/main，出数据用，用完删） |
| 老二进制 target | `CARGO_TARGET_DIR=C:/Users/test/Desktop/work/mc-drill/target-old`（**隔离**，防指纹来回全量重建污染主 target） |
| 新二进制 | 主 checkout 直接 `cargo run -p nomifun-web`（主 target 已暖） |
| 数据目录 | `C:/Users/test/Desktop/work/mc-drill/data`（**绝不**指向真实 dev 库——本机真实库是 069–088 旧谱系，新二进制首次启动会触发退役，别把它卷进来） |
| 端口 | 8791（老）/ 8792（新），先跑 `bun scripts/free-ports.mjs 8791 8792` |
| 通道 | 两个进程都显式 `NOMI_CHANNEL=dev` |
| DB 检查工具 | bun:sqlite 脚本（家规先例 absorb-8 quality.db；本机无 python） |
| 凭据 | **零 LLM 依赖**：活课程用 tutorial pack 走 import API（`assets/tutorial/course.json`，v3 双时代同形），不跑生成管线 |

## 相位 A：main 时代出数据（老二进制，001–065）

1. 老代码 worktree 里 `cargo build -p nomifun-web`；核产物 mtime 后启动：
   `NOMI_CHANNEL=dev CARGO_TARGET_DIR=... cargo run -p nomifun-web -- --data-dir C:/Users/test/Desktop/work/mc-drill/data --port 8791 --api-only --insecure-no-auth`
   （`--data-dir` 放子命令前，家规；insecure-no-auth 双时代支持，免 token）。
2. 记录 effective user_id：任一 user-scoped 响应或 DB `users` 表取（后续断言
   全按此 user 圈定）。
3. **造数据（全走老代码 API 真路径）**：
   - `L1` 活传统课：`POST /api/learning/courses`（body = tutorial pack JSON）
     导入 → 记 `course_id` → `POST .../enroll` → 打开课时（GET lesson）→ 对
     至少两个活动 `POST` 作答（progress 出 completed/in_progress 态）。
   - **卡面确认**：`GET /api/learning/reviews/due` 须出现 L1 的卡（review_items
     产生时机以 A 相位实测为准——若导入/作答后仍无卡，读老代码
     `service/review.rs`/`lesson.rs` 找到出卡触发点补一步；A 相位结束的硬条件
     = `learning_review_items` 中 L1 卡数 > 0）。
   - `L2` 待删传统课：同法再导一份 pack（或同 pack 重复导入），enroll + 出卡
     > 0 → `DELETE /api/learning/courses/{L2}`，body `{"delete_reviews": false}`
     （main 时代 UI 默认路径）→ **课程行消失、卡保留**，DB 确认。
   - `G1` 旧图课（推荐，验 §1 全清）：老 API 建 `course_kind='learning_graph'`
     课（描述流）；若老 API 建图课路径不便，bun:sqlite 对 001–065 schema 手插
     一棵最小图课子树（courses/modules/lessons/activities/review_items，
     course_kind='learning_graph'）——§1 行为本就有基线测试钉，真机此相
     位为 E2E 加演，允许手插。
   - `P1` 个人面：建一道自建题（custom questions API）+ 对一张 L1 到期卡作答
     一次（产生 `learning_review_log`/`learning_review_events`/attempt）+ 打卡
     API 一次（`learning_checkins` 落行）。
4. **冻结快照**（evidence/phaseA/）：
   - 停服务；`data` 目录整份拷贝为 `data-001-065-frozen/`（升级前铁证）。
   - bun:sqlite dump 脚本输出 `snapshot-A.json`：15 张相关表（courses/modules/
     lessons/sections/activities/enrollments/progress/review_items/attempts/
     review_log/review_events/question_tags/course_tags/checkins/
     custom_questions）的全行 JSON（按 course_id 分桶）+ counts。
   - 由 A 快照**算出期望值**写 `expected.json`：L1 各表行数、
     L1 到期卡数（due_at<=now 且未归档/未挂起）、events/checkins 计数、
     L2/G1 各表应清零清单。

## 相位 B：新二进制升级（feat/learn0928 尖端 03a0c5380）

1. 主 checkout `cargo run -p nomifun-web -- --data-dir <同一 data> --port 8792
   --api-only --insecure-no-auth`。
2. 启动日志断言（`data/logs/`）：
   - 迁移按序应用 066 → 067 → 068 → 070，**070 成功**；
   - **零** retired-datasets / 谱系拒绝字样（真前缀路径必须走通）；
   - orphan audit 零 require-parent 级 finding（sweep 后不应有）。
3. 服务健康：`GET /api/learning/courses` 200。

## 相位 C：断言（bun:sqlite + API 双面）

**SQL 面**（dump-C.json 与 expected.json 逐项比对，脚本出 PASS/FAIL 清单）：
- **清扫面**：L2、G1 为键的 11 表（enrollments/modules/lessons/sections/
  activities/progress/review_items/attempts/review_log(source=course)/
  question_tags(source=course)/course_tags）计数全零；LEFT JOIN 悬空课时/模块/
  注册 = 0。
- **保全面**：L1 十表行数 == A 相位；L1 内容字节守恒（dump 逐行 diff
  config_json/body_md/points 等内容列）；自建题及其 review_log(source=custom)
  原样；`learning_review_events` 与 `learning_checkins` 行数**不变**（含 L2 卡
  的作答流水——打卡统计不随卡删契约）。
- **互斥面**：courses 表内 course_kind 只剩 'traditional'（'learning_graph'
  归零）。

**API 面**（新端口 8792，curl）：
- 课程列表：含 L1，不含 L2/G1；
- `GET /checkins/today`：due_count == expected（**只有 L1 的卡**）；
- `GET /reviews/due?due_only=true`：全部行属于 L1，无 L2 卡；
- 问题管理列表：零 L2 行；
- `GET /stats/memory`：零死课程条目；
- 打开 L1 课时 → 200；对一张 L1 卡作答 → 200 且 `checkin today` 相应推进
  （升级后写路径健康）。

**可选 UI 面**：`bun run dev:web`（Vite 指向 8792 后端）目验课程列表/打卡页/
L1 课时/复习四屏——或 browser-use 脚本过。不阻塞判据 1–6。

## 证据与收口

- 产物全落 `mc-drill/evidence/`：frozen 数据目录、snapshot-A/C、expected、
  断言输出（逐条 PASS/FAIL）、B 相位迁移日志摘录。
- 全 PASS → 回写 m2 票 Resolution「真机」段（日期 + 判据逐条 + 证据路径），
  票即真机闭环；任何 FAIL → 按票重开，证据目录保留。
- 清理：删 allo-drill-main worktree + mc-drill/target-old（几 GB）；data 与
  evidence 保留至票面回写完成后可弃。

## 风险与预案

| 风险 | 预案 |
| --- | --- |
| 老二进制端口/flag 与现款不同 | 一律以老 worktree 自带 `scripts/free-ports.mjs` + 其 main.rs flag 定义为准，先 `--help` 核对 |
| review_items 出卡触发点不明 | A 相位硬条件兜底（卡数>0 才准冻结），触发点按老代码实测定 |
| 迁移中途失败 | SQLite 单迁移事务回滚，data 目录作废重跑相位 A（frozen 副本保留作证据） |
| 误指真实数据目录 | 脚本首行断言 data 路径前缀必须是 mc-drill/，否则拒跑 |
| 新二进制 warm target 被 drill 污染 | 老二进制用隔离 target-old，新二进制只读复用主 target，不交叉 |
