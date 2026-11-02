# 施工图 · x09+x10 扁平信封模式与 FSRS 算法版本

## 1. 元信息

- **目标**：学习者状态 JSON 文档列获得版本语义——单对象列用扁平 `{version, …}` 信封、多载荷行用行级 `doc_version`；FSRS 用户参数加 `algo` 包装，未知算法回退默认；review_log 记算法代际
- **版本**：v1
- **状态**：草稿（待用户确认后定稿）
- **日期**：2026-11-02
- **覆盖票**：x09（优先 1 限定）、x10；依赖图 1（基线节）；x20/x16 为本图阶段二回写位

## 2. 现状摘要

- **范本**：`CourseSourcePlanSnapshot`（generation/snapshot.rs:28-39，`version: u32` + `COURSE_SOURCE_PLAN_SNAPSHOT_VERSION`）→ 读侧 `parse_source_plan_snapshot`（service/course.rs:1102-1120）三态 `Available / BrokenShape / UnknownVersion`，视图降级 `available=false`（:1321-1329）。这是全仓唯一的版本分支，也是本图 helper 的行为范本。
- **错误处理三套口径并存**（无统一 helper）：硬错 `map_err(internal)`（config_json 13 处读点、blueprint/samples、basis anchors、mounts svc:516）；宽容 `.ok()/unwrap_or_default()/else continue`（report_json、node_ids_json、source_paths_json、samples_meta_json、aliases、mounts 多数读点）；版本分支仅 course.rs 一处。`deny_unknown_fields` 学习 crate 零使用（verify_math.rs:1043-1057 为移除证据）。
- **逐列事实**（读写点、serde 形状、CHECK）见 2026-11-02 图 5 读码底稿 B 节。关键：`quiz_rounds.plan_json` 是**对象数组**（CHECK `json_type='array'`，quiz.rs:352-363 写 / :439-440、:940-965 读，坏形 → Internal）；`report_json` 读侧 `let Ok(..) else continue`（quiz.rs:587-604）；`answers_json`、`attempts.response_json`、`repairs.ops_json` **零读点**；`aliases_json` 双读旧/新形（alias_records.rs:38-82）+ 三处 SQL `json_each COALESCE` 双读（:156-158、svc:3117-3119/:3205/:4819）；basis 三列纯字符串数组（parse_anchor_row 硬错 svc:4432-4446，lesson.rs:1567-1581 warn+跳过）。
- **x10 事实**：`SchedulerSettings` 无 serde（scheduler.rs:19-28）；设置读取 `checkin.rs:7-54`——`fsrsParameters` 按 JSON 数组解析（`as_array → as_f64`，无长度校验）；`w[20]`/`FSRS6_DEFAULT_DECAY`（scheduler.rs:112-116，fsrs crate 6.6.1）；`FSRS::new` 失败静默回默认（:148-151）；前端 `LearningSettings.tsx:27-118` TextArea 逗号切分、无个数校验；`configKeys.ts:166` 类型 `number[]`；review_log 写点 review.rs:1755-1804（十二列具名 INSERT）+ 合成行 progress.rs:679-692。

## 3. 总体结构

```mermaid
graph LR
  write[写路径] -->|单对象列| env["{\"version\":N, …载荷}"]
  write -->|多载荷行| rowver[行级 doc_version 列]
  read[读路径] --> rd[read_envelope helper\nversion 分支：v0 裸形 / 当前 / Unsupported]
  rd -->|当前| ok[正常解析]
  rd -->|Unsupported| degrade[逐列显式降级，禁止猜形]
```

- **本票范围（优先 1，Q1 门槛）**：`quiz_rounds`（行级 doc_version）、`concept_registry.aliases_json`（行级 doc_version）、`structure_repairs.ops_json`（对象信封）、`learning_attempts.response_json`（对象信封）。**豁免（记档 x20）**：`basis_declarations` 三列、`lessons.source_paths_json`、`growth_batches.node_ids_json`、`source_kb_ids`——纯字符串数组，演进规则=只许追加元素。**阶段二（后续 PR，不在门槛）**：`config_json`、`samples_meta_json`、`mounts_json`、`blueprint_json/samples_json`。
- **全局不变量与禁区**：
  - 信封扁平：`version` 与载荷键并列（现存两信封同形，Q7）；不引入 `data` 包装层。
  - **禁止猜形**：Unsupported 不是错误也不是宽容解析，是显式降级路径，逐列定义（4.2）。
  - 历史裸对象（无 version）= v0，读通，不回填。
  - 列上的 `json_valid/json_type` CHECK 全部保留；本票不为信封改任何已发布列的 CHECK。
  - x10 不动 `desiredRetention`/`tzOffsetMinutes` 的形状；不改 fsrs crate 版本。

## 4. 分块图纸

### 4.1 · 信封 helper（新文件 `src/envelope.rs`，learning crate 顶层）

- **接口契约**：

```rust
#[derive(Serialize, Deserialize)]
pub struct FlatEnvelope<T> { pub version: u32, #[serde(flatten)] pub payload: T }

pub enum EnvelopeRead<T> { Bare(T), Current(T), Unsupported(u64) }

/// 单对象列读侧：先按信封形解析，失败回退裸形（v0）；version 不识别 → Unsupported
pub fn read_envelope<T: DeserializeOwned>(raw: &str, current: u32) -> Result<EnvelopeRead<T>, serde_json::Error>

/// 写侧：恒打当前版本
pub fn write_envelope<T: Serialize>(current: u32, payload: &T) -> serde_json::Result<String>
```

- **失败路径**：解析失败（真坏 JSON）→ 维持各列现有口径（硬错列报 Internal、宽容列走宽容），helper 不改变错误面，只新增「版本」这一个维度。

### 4.2 · 优先 1 逐列落点

| 列 | 基线编辑 | 写侧 | 读侧与 Unsupported 降级 |
|---|---|---|---|
| `quiz_rounds`（plan/answers/report） | §7 CREATE 加 `doc_version INTEGER NOT NULL DEFAULT 1 CHECK (doc_version >= 1)`；常量 `QUIZ_ROUND_DOC_VERSION=1` | INSERT/UPDATE 三点 bind 1 | `load_round_plan/load_round_row`：`doc_version != 1` → `AppError::Internal("quiz round doc v{n} unsupported")`（进行中回合是活动状态，错误优于静默）；`quiz_calibration`：版本不符 → `warn` + `continue`（统计面跳过） |
| `concept_registry.aliases_json` | §2 CREATE 加 `doc_version INTEGER NOT NULL DEFAULT 1`；常量 `ALIAS_DOC_VERSION=1` | `write_alias_records`/铸名 INSERT/`'[]'` 三点随行写 1 | 相关 SELECT 增取 `doc_version`（5 读点，底稿 B3 清单）；`!= 1` → 按 `parse_alias_records` 返回空 + `warn`（别名池空 = 铸名门走「无候选」分支，可见降级） |
| `structure_repairs.ops_json` | §6 CHECK `array` → `object`（未发布列，随形改） | svc:967-989 改 `write_envelope(1, &OpsLog{ops: ops_log})` | 零读点；x20 登记形状 |
| `attempts.response_json` | 不动（015 已发布，`json_valid` CHECK 兼容 object） | progress.rs:198-213、review.rs:1560-1575 改 `write_envelope(1, &ResponseDoc{response})` | 零读点（x16 阶段二补读侧判别时消费） |

- `parse_anchor_row`/basis 三列/其余数组列：零改动，x20 登记为 `envelope=array-exempt`。

### 4.3 · x10 FSRS 参数包装

- **数据形状**：`learning.fsrsParameters` 新形 `{"algo":"fsrs6","w":[21 个 f32]}`；常量 `pub const FSRS_ALGO: &str = "fsrs6";`（scheduler.rs）。
- **接口契约**：

```rust
// scheduler.rs
pub enum FsrsParams { Fsrs6(Vec<f32>), Unsupported(String) }   // Unsupported 携带 algo 原文
pub fn parse_fsrs_params(v: &serde_json::Value) -> FsrsParams
//  Value::Array → Fsrs6（现状逐 f64 过滤逻辑原样）
//  Object 且 algo=="fsrs6" → Fsrs6(w)（w 校验同现状：非空/全 finite）
//  Object 其它 algo → Unsupported(algo)
impl SchedulerSettings { pub fn decay_component(&self) -> f32
//   Fsrs6 → w.get(20).copied().unwrap_or(FSRS6_DEFAULT_DECAY)；Unsupported/空 → 默认 }
```

- **调用点**：`checkin.rs:35-49` 改走 `parse_fsrs_params`（`Unsupported` → `tracing::warn!("fsrs params algo {} unsupported, falling back to defaults", algo)` + 不赋 parameters）；`predicted_retrievability` 改 `decay_component()`；`FSRS::new` 失败静默回默认处补同款 warn。
- **落库**：基线追加 §12：`ALTER TABLE learning_review_log ADD COLUMN algo TEXT;`（052 已发布 → 基线承载；NULL=历史行=fsrs6 口径）。`record_review_log` 真实行 bind `FSRS_ALGO`，合成行（progress.rs:679-692）bind `NULL`。
- **前端**：`configKeys.ts:166` 类型改 `number[] | { algo: string; w: number[] } | undefined`；`LearningSettings.tsx` 载入时旧数组原样显示、新形解 `w` 显示；`saveParameters` 恒存 `{"algo":"fsrs6","w":[…]}`；`fsrsParametersHint` 文案补「算法标签自动附加」。未知 algo 载入 → 输入框置空 + 提示「参数来自不支持的算法，已回退默认」（i18n 双语新 key）。
- **失败路径**：`Unsupported` 永不 panic、永不半生效（parameters 整体回默认）；review_log 的 `algo` 只是标注，不参与调度分支。

## 5. 决策卡片

### A1 · 多载荷行用行级 `doc_version`，不给数组列硬套对象信封

- **类型**：数据结构
- **决策**：`quiz_rounds` 与 `concept_registry` 加行级 `doc_version` 列；单对象列（ops/response）用 `FlatEnvelope`。
- **理由**：`plan_json` 是对象数组，列 CHECK 钉 `array`——套 `{version,…}` 信封要改 CHECK 形并波及三处 SQL/直读测试（tests.rs:8211-8228、:8436-8443）；一行三载荷共享一个代际（plan/answers/report 同生同灭）语义也更准。registry 的 `json_each` SQL 双读三处同理不动。
- **代价**：两种版本载体并存（列内 version / 行级 doc_version），x20 契约总表必须逐列标注用哪种。
- **推翻条件**：阶段二给 `config_json` 等补版本时若发现行级/列内混用造成误读，统一收拢到一种。

### A2 · Unsupported 逐列定义、禁止统一「当 v0 读」

- **类型**：失败模式
- **决策**：版本不识别时的行为逐列写死（4.2 表）：活动状态面报错、统计面 warn+跳过、候选面降级为空+warn。
- **理由**：学习者状态被新版本写过 → 旧版本读到时**猜形比报错危险**（半解析会污染结算/校准）；「逐列显式」让每个消费面按自己的容错职责选降级，与 source plan 的 `available=false` 同哲学。
- **代价**：四列四套降级要各自测试；新列接入时容易忘选——helper 的枚举强制调用方 `match`，编译期兜底。
- **推翻条件**：出现「同列既要报错又要跳过」的第二消费面冲突时，把降级策略上移为参数。

### A3 · fsrs 未知算法整体回退默认（不是只回退 w[20]）

- **类型**：失败模式
- **决策**：`Unsupported(algo)` → `parameters` 整体置空走库默认 + warn；`decay_component` 与 `FSRS::new` 同源。
- **理由**：w 向量布局与算法绑定，混搭（新 algo 的 w 配旧位置语义）比全默认更危险；review_log 的 r_pred 快照已按次落列，回退期的预测误差可被校准面观察。
- **代价**：升级跨算法时用户自定义丢失（可感知，前端有显式提示文案）。
- **推翻条件**：出现第二算法实装（届时 `FsrsParams` 加变体，`decay_component` 分支化——本枚举就是为它预留的形状）。

## 6. 施工顺序

| # | 施工步 | 图纸位置 | 验收 |
|---|---|---|---|
| 1 | `envelope.rs` helper + 单测（信封形/裸形回退/Unsupported 三态） | 4.1 | `cargo test -p nomifun-learning` 新单测绿 |
| 2 | 基线 §7/§2/§6 编辑（doc_version ×2、ops CHECK 改 object、常量） | 4.2 | `cargo test -p nomifun-db` 全绿（表数契约不变）；Q8 fixture 过 |
| 3 | 四列写侧改信封/doc_version + 读侧版本分支与降级 | 4.2 | 每列一组红绿样：v0 裸形读通 / v1 读通 / v2 显式降级（quiz 报错、calibration 跳过、aliases 空+warn）；既有 quiz/alias/basis 测试全绿 |
| 4 | x10：parse/decay/algo 列/前端设置页 | 4.3 | 单测：裸数组（旧形）读通、新形读通、`{"algo":"fsrs9"}` 回退默认+warn；`learning-smoke xp` 档 EXIT=0；设置页手工过（保存→重载→提示三态） |
| 5 | x20 阶段二回写位记入本图 §8（不实现） | 1 | — |

## 7. 未决事项

- **阶段二清单**（x09 后续 PR）：`config_json`（13 读点换 `read_envelope`）、`samples_meta_json`、`mounts_json`（parse 已吃 `{mounts:[…]}` 形，只差 version 检查）、`blueprint_json/samples_json`（可再生，或直接归 x19 懒再生）。影响 4.2。推荐默认：x19 落地时一并做。触发重议：阶段一合入后出现针对这些列的格式变更需求。
- **`doc_version` vs 信封内 version 的常量命名**：统一 `*_DOC_VERSION` / `*_SCHEMA_VERSION` 前缀由 x20 定总表时收口。推荐默认：行级用 `_DOC_VERSION`、列内用 `_SCHEMA_VERSION`。

## 8. 变更记录

| 日期 | 类型 | 位置 | 变更 | 根因/触发 |
|---|---|---|---|---|
| 2026-11-02 | 修订 | 3/4.2 | basis 三列由「必补信封」改「豁免（纯字符串数组）」——与 source_paths/node_ids 同规则；x09 票面随后同步 | 读码底稿 B2（三列均 `Vec<String>` 无结构体）；成图走查发现「给字符串数组套信封」只有成本没有读者 |
| 2026-11-02 | 修订 | 4.2 | quiz 信封载体从「列内扁平信封」改为「行级 doc_version」——plan_json 是数组，列 CHECK 钉 array | 读码底稿 B1 + 反模式自查（抉择点必须展开） |
