# s1: 造数脚本 + 真机走查清单

## Status

已关（2026-11-11）。副本验证全绿：INSERT 全过现行迁移链 CHECK（UUIDv7/
评分/区间）、读数 8 卡到期 + quiz 8/6 due、日历 4 天 × 54 XP、打卡史 4 日、
review_log 14 行全 'self' 合规；幂等重跑跳过；`--fresh` 清零残留（孤儿行
0）。真机走查待用户执行（见下）。

## Terms

- **R1（脚本）**：`scripts/seed-learning-triggers.mjs`（bun:sqlite）：
  - 目标库防呆：缺省 = `<数据根>/Flowy/Nomi-dev/flowy-backend.db`（三平台
    镜像 `default_data_dir` + dev 通道后缀）；`--db` 可覆盖；目标不存在即
    死。库被锁（后端在跑）→ 报错提示先关全部实例。
  - 写前自动备份：`VACUUM INTO '<db>.pre-seed-learning-<ts>'`，锁探测二合一。
  - 目标人：`installation_identity`（singleton='installation'）→ owner_user_id。
  - 合成课：`launch-sim 触发数据`（幂等标记 = 该标题），1 模块 8 课时
    （est 25、content_generated=1），每课时 1 客观题，题型覆盖
    single_choice/true_false/fill_in_blank/multi_choice/numeric/ordering/
    matching——配置全部通过真实 `evaluate()` 判分语义（answer 形态逐题型
    对齐）；报名 owner。
  - 结算史：8 笔 `node-completed`（full：earned 27 = 25+2 bonus），ts 散布
    过去第 4–7 天本地日间（每天 2 节点 = 54 XP ≥ 40 目标）→ quiz due
    （8/6）+ XP 日历史 + 打卡史原料。
  - 复习卡：8 张——4 张裸卡逾期 1/2/4/7 天（review_count 0）；4 张推进史卡
    （review_count 2–5、stability 3.5–26、difficulty 4.5–8.2，其中 2 张带
    lapse），due 逾期 0.5–2 天；推进史卡配 `learning_review_log` 行
    （rating_source 'self'、rating 1/3、elapsed_days/r_pred/review_day/
    algo='fsrs6' 齐全，首推 stability/difficulty 前值 NULL）→ 排序/
    可提取性分桶/弱先/记忆健康有信号。
  - 打卡史：过去 4 天各 1 行 `learning_checkins`（goal 40、reviewed_count
    54）；今日不造——留给真实复习/测验结算去锁。
  - **不造**：quiz-completed（保 quiz due）、custom questions、今日打卡。
  - `--fresh`：清标记课全部衍生行（xp events/checkins/review log/items/
    activities/lessons/module/enrollment/course）后重造；幂等重跑默认跳过。
  - `--dry-run`：只打印计划不写库。
- **R2（读数）**：脚本收尾打印验真读数——到期卡数、已结算节点 8/6（due
  翻转）、过去 4 天 XP 与打卡日、quiz 进度口径查询结果。
- **R3（走查清单）**：Resolution 附真机走查清单（复习横幅→复习会话→打卡
  锁定；dev:web curl quiz progress/start/grade/complete 全链）。

## Acceptance

- 在开发库**副本**上跑通：脚本 EXIT 0、读数符合 R2、真实后端迁移态兼容。
- 幂等：连续两跑第二跑跳过不重复；`--fresh` 后重造读数一致。

## Resolution

### 用法

```bash
# 关闭所有实例后（脚本备份步即锁探测，后端在跑会失败并提示）
bun scripts/seed-learning-triggers.mjs            # 默认开发库
bun scripts/seed-learning-triggers.mjs --fresh    # 清旧模拟课重造
bun scripts/seed-learning-triggers.mjs --dry-run  # 只打印计划
```

写前自动备份 `<db>.pre-seed-learning-<ts>`；目标人 = installation owner。

### 真机走查清单（上线验证）

1. **复习面**：`bun run dev`（或 `dev:web`）→ 学习页应出现「今日复习 · 8
   张到期」横幅 → 进复习会话逐题作答（答案：光速=30 万公里/秒；素数=假；
   内角和=180；无理数=π/√2/e；12×8=96；单位排序=毫米→厘米→分米→米；
   配对=氢H/氧O/碳C；水=H₂O）→ 自评评分 → 完成后队列清空。
2. **过滤面**：复习过滤器按课程/标签/其它筛——模拟课应单列可选。
3. **XP 日历/统计**：过去第 4–7 天（本地日）各 54 XP、两个 node 点；4 个
   已锁定打卡日（20261104–07，各 54/40）。
4. **记忆健康**：8 张卡可提取性分布（4 张裸卡 + 4 张带 stability 3.5–26、
   其中 2 张含 lapse）。
5. **打卡锁定**：今日初始未锁（造数不碰今天）；今日 XP ≥ 40 锁定。纯模拟
   课复习（8×3=24）+ 一轮 quiz（≈13–15）≈ 39 上下贴线——差一点时再真学任
   何真实课的一个节点（+27）即过线。**注意**：造数不给今日结算——同卡同日
   幂等锚会吞掉真实推进，今天必须由真实复习/测验落账。
6. **测验点击级走查**（quiz-ui 波 u1–u5 已落地，原 API 级清单升级）：
   - 课程工作区「知识检测」卡：未 due 显「距上次检测 N/K 次学新结算」+
     禁用态；due（8/6）显「开始检测」可点；
   - 开测 → Modal「正在组卷，请稍候…」等待文案（有凭据时变式链 LLM 在跑，
     等待窗口如实呈现；零凭据 = bank 旧题直接出）→ 逐题作答（题面带题源
     节点标注）→ 提交即判分（对/错 + 判底 + 解析 + 证据）→「下一题」；
   - 全卷作答 →「交卷结算」→ 结算报告：总净入账 + 逐节点对账行 +
     分级（excellent/pass/half/low 四档）→「完成」回课程页 →
     知识检测卡 N/K 归零（0/6 积累态）+ XP 日历出现 quiz 入账；
   - API 级复核（可选）：`GET /api/learning/courses/<course_id>/quiz/
     progress` → due=true → start/grade/complete 全链（course_id 见脚本
     收尾打印）。
7. **收尾**：验证完可 UI 删课（账本按设计保留日历史）或 `--fresh` 重建。

### 已知偏离（如实）

- 造数直插账本，不过 companion XP 桥——companion 余额无此历史，学习面
  （日历/打卡/触发）不受影响。
- `--fresh` 的打卡清理按指纹（goal=40 且 reviewed_count=54）匹配，理论上
  可误删同日同指纹的真实打卡——仅开发库工具，风险已接受。
- 题目推进史（stability/lapse 序列）是手工合理值，非 FSRS 重算——只喂
  呈现/排序面，不参与调度真值（真值在真实推进时重算）。

### 验证留痕（副本，2026-11-11）

开发库一致性快照（VACUUM INTO 只读快照）上：首跑 EXIT 0 读数 8/8/0/4；
重跑跳过；`--fresh` 后 1 课/4 打卡/孤儿行全 0；XP 本地日分桶 1104–1107 各
54；review_log rating_source 全 'self'、rating ∈ [1,4]、r_pred ∈ (0,1]；
八题型 answer 形态逐一对齐 `evaluate()` 判分语义。
