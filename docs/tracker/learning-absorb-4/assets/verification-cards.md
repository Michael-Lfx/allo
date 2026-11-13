# 四轮吸收 k01–k07 双臂真机对照卡（2026-10-23）

> 方案见 [map.md](../map.md) 「验证总纲」。同二进制开关臂 = `nomicore-new.exe`
> （集成臂 tip 构建，mtime 2026-10-23 14:01）；版本臂 = 旧 tip 二进制
> （mtime 2026-10-22 17:21 的构建，k06 前资产；副本 mtime 13:17）vs 同一新二进制。
> 运行根目录：`C:/Users/test/Desktop/work/absorb4-verify/`（各臂自带
> `run.db` 副本 + `data/encryption_key`；基库 = 2026-10-22 verify 库，带凭据）。
> 观测存档：`<臂>/data/observation/learning-smoke/run-*/calls.jsonl`。

## k01 Mindmap 负坐标翼（版本臂，文字实录）

同一 fixture（3 主干 / 6 块，两翼非空）下旧算法复刻 vs 新 `computeLayout`：

| 读数 | 旧（修复前算法） | 新（中心对称） |
|---|---|---|
| 画布 width | 1230 | 1270 |
| 全部矩形 x 范围 | **[-595, 705]** | **[40, 1230]** |
| 左翼块 x 范围 | **[-595, -445]（整翼在负坐标区 = 视口裁剪不可见）** | 全部 ≥40 |
| 中心框 x | [525, 705] | [545, 725] |
| 中心框与翼首列块重叠 | **是（右翼 445–595 与中心框 525–705 重叠 50px）** | 否（右翼块 min_x 821 > 725） |
| 单测 | — | 3 passed（画布内 / 不压中心框 / 单翼） |

结论：旧臂缺一翼且中心框压另一翼首列；新臂全部节点落画布内。单测文件
`ui/src/renderer/pages/learning/components/structure/mindmapLayout.test.ts`。

## k02 findings 水位 + 自动维护（开关臂）

`learning-smoke maintenance`，同二进制两臂：

| 读数 | on（`NOMI_LEARNING_FINDINGS_WATERMARK` 未设） | off（`=off`） |
|---|---|---|
| 纯读 needs（两臂相同） | unconsumed=2 pending_mounts=1 stale=true needs_repair/mount=true | 同左 |
| 归档课扫描面 | scanned=1（归档课不扫） | scanned=1 |
| 回合 1 | repair=noop, mount=mounted | repair=noop, mount=mounted |
| 水位 | **Some(2)** | **None（不推）** |
| 回合 2 | idle=1 worked=0，**LLM 2→2（零调用）** | worked=1，**LLM 2→3（全量重读重跑）** |
| EXIT | 0 | 0 |
| 设置读侧 | enabled=true interval=45（写入值） | 同左 |

存档：`artifacts/maint-on/run.log`、`artifacts/maint-off/run.log`。

## k03 est 时长预算行 + capacity_note（开关臂）

`learning-smoke content`，同 fixture：

| 读数 | on | off（`NOMI_LEARNING_EST_BUDGET=off`） |
|---|---|---|
| user 回合含注入行 `- 时长预算：` | **1** | **0** |
| 发布观测 `capacity_note` 事件 | 0（未自报，缺席合法） | 0 |
| 课时读数 | sections=5 activities=5 body_chars=3379 | sections=5 activities=5 body_chars=1693 |
| EXIT | 0 | 0 |

存档：`artifacts/content-on/run.log`、`artifacts/content-off/run.log`。
说明：capacity_note 是自愿自报，两臂模型均未写（非门禁，learnhub 同款零拦截）。

## k04 题面相似度防雷同（开关臂）

`learning-smoke quiz`：

| 读数 | on | off（`NOMI_LEARNING_QB_SIMILARITY=off`） |
|---|---|---|
| pool_checks | **6**（门在执法） | **0**（门不在） |
| sim_hits | 0 | 0 |
| dropped / retries | 0 / 0 | 0 / 0 |
| variants_generated | 6 | 6 |
| EXIT | 0 | 0 |

存档：`artifacts/quiz-on/run.log`、`artifacts/quiz-off/run.log`。
说明：变式按设计是「同考点换角度」，on 臂 6 次过池全部低于 0.7——门不误伤
真实换角度题；真机未自发出现雷同题（sim_hits=0 如实记录，fixture 无区分度）。
换数/雷同的机械红绿由单测锁死（同题换数新门红、旧门绿）。

## k05 内容纪律 R10–R13（开关臂）

同上 content 两臂：

| 读数 | on | off（`NOMI_LEARNING_CONTENT_DISCIPLINE_V2=off`） |
|---|---|---|
| R10 具象替身先 | 1 | 0 |
| R11 受众语域 | 1 | 0 |
| R12 表述工具在场 | 1 | 0 |
| R13 断言-例子一致 | 1 | 0 |
| j07 基线条款（先推后练/对照要真对照） | 在场 | 在场（回归护栏） |

人读效果判据（同 fixture 各一次采样，直接读落库节正文判）：

| 条款 | on | off |
|---|---|---|
| R10 抽象首讲有具象替身 + 规则配「为什么」 | ✅ s1 先给「两只股票一日内小幅挪动 vs 涨 4%/跌 3%」的情境量 + 收益分布 SVG，正式公式推迟到 s2；s2 用「为什么开方、为什么除 n−1」借替身解释 | ⚠️ s1 以「分布」抽象框定后**直接给出标准差公式**，无量值替身；s2/s3 才补年化与算例 |
| R11 情境语域匹配声明受众 | ✅ 交易日/结算单/持仓体验 | ✅ 资产/样本/估计值（同域，无低龄情境） |
| R12 记号即兴使用 | ✅ Alpha 首现即括注正名「对数收益率」，σ 随定义引入 | ✅ Alpha 前节已引入，σ 随定义引入 |
| R13 例题数字能推出断言 | ✅ s3 例 1：{+1%,+3%} → 均值 2%、平方和 2%²、/(n−1)=1、开方 1.41%（复核通过） | ✅ s3 例 1：(−0.02,0,0.02) → 0.0008/2 开方 0.02（复核通过）；例 3 年化 0.012×15.87≈0.19 复核通过 |
| j07 计数器（naming/narrative） | 无新增观察（两臂同一观察条口径） | — |

说明：on 臂在 R10 上呈现明确差异（替身先于形式化），其余三条两臂均未违规；
单次采样不构成统计，机械证据仍是条款短语进入实际 system 提示词（1 vs 0）。

## k06 分解/地形/tier（版本臂，跨构建）

`learning-smoke structure --goal "掌握一元二次方程的解法与判别式应用"`：

| 读数 | 旧 tip 二进制 | 新二进制 |
|---|---|---|
| EXIT | 0（首跑撞 flash 超时重试一次后成功） | 0 |
| spines / blocks / edges | 8 / 52 / 93 | 9 / 54 / 83 |
| assumes / composes / contrasts | 63 / 20 / 10 | 65 / 13 / 5 |
| gaps | 4 | 5 |
| doors | 7 | 7 |
| 门措辞含「进课前/进入课程前」 | 0 | 0 |

人读条款判（两臂并排）：

| 条款 | 旧 | 新 |
|---|---|---|
| assumes 是否逻辑前提（抽掉后继无法进行） | ✅ 抽样 3 条（二次项系数/一次项系数/常数项 → 一元二次方程的定义）成立 | ✅ 抽样 3 条（一般形式 → 定义，二次项系数 → 一般形式）成立 |
| 主干链有无悬空环节 | 未发现（assumes 端点全部可解析为机械绿；gaps 4 条记录盲区） | 未发现（端点全解析；gaps 5 条） |
| 门措辞无「进课前须具备」类进门前提 | ✅（7 门全为「能…」可自检句） | ✅（7 门全为「能…」句） |

说明：本轮两臂都产出合法结构且无进门前提措辞；差异体现在条款措辞已入
提示词 + 金样锁定（机械闸），产物统计差异（edges 93→83、gaps 4→5）如实
记录不作优劣判定（LLM 随机性）。

## k07 评审判据①挂号子查（开关臂）

`learning-smoke grow --goal ... --batches 2`：

| 读数 | on | off（`NOMI_LEARNING_REVIEW_V2=off`） |
|---|---|---|
| EXIT | 0 | 0 |
| 节点数 | 9 | 10 |
| 评审调用（存档） | 1 | 2 |
| 存档评审提示词含 V2 子查短语 | **1** | **0** |
| 存档评审提示词含旧并句「三处皆无的名词」 | 0 | **2** |
| 评审 findings 落库 | 0 | 0 |

存档：`artifacts/grow-on/run.log`、`artifacts/grow-off/run.log`。
说明：两臂评审均零 findings（批次本身干净），fixture 对内嵌上位词无区分度
——如实记录，不硬造；新措辞确已进入真机评审提示词（机械证据）。

## 回归护栏与真机纪律

- 既有 smoke 判据：以上十臂 EXIT 均 0（k06 旧臂首跑因 flash 超时重试后
  EXIT 0，第二次重跑一次通过）。
- 构建：`NOMI_CHANNEL=dev` 显式带；`--data-dir` 放子命令前；跑前核
  `nomicore.exe` mtime（旧二进制已冻结副本）。
- 观测存档路径随各臂日志打印（`calls.jsonl` + `learning-tasks.jsonl`）。
