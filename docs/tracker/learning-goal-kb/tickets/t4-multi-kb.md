---
id: t4
title: "多知识库扩展：数组落库 + 预算分摊 + 归属引用 + 前端多选"
type: task
status: closed
assignee: zhangyj
blocked_by: [t2, t3]
part_of: ../map.md
---

# t4: 多知识库扩展：数组落库 + 预算分摊 + 归属引用 + 前端多选

**What to build:** 建课支持选择**多个知识库**（用户原始诉求）。检索侧 `search_bases` 与
t3 的读库面已天然多库（`kb_ids: &[KnowledgeBaseId]`），本票是数据面与输入面的参数扩展：
`learning_courses.source_kb_id` 单列 → 来源库列表；采样预算按库分摊；引用/快照带库归属；
前端单选 → 多选；原文回看按归属解析。

**Blocked by:** [t2 采样](t2-goal-aware-sampling.md)、[t3 写作接库](t3-lesson-live-retrieval.md)
（多库是这两者的参数化，不单独建设）。

- [x] **落库**：`learning_courses.source_kb_ids TEXT NOT NULL DEFAULT '[]'`
      （`json_valid` 数组校验，迁移 **073**）；首个 = 主来源；`source_kb_id` 列
      **保留不动**（主来源 = 列表首个 = 该列），`id_schema_contract` 的 SetNull
      条目不动——引用完整性移应用层读侧（读侧剔除已删库 id）；老数据回填
      （`source_kb_id` → 单元素数组）。
- [x] **请求面**：`GenerateCourseRequest` 增 `knowledge_base_ids: Vec<KnowledgeBaseId>`
      （兼容单 `knowledge_base_id`：列表优先、同时给出时单值必须是列表成员）；逐库
      核验存在；**选择数量上限 5**（超限 400）。
- [x] **采样分摊**：t2 的目标感知采样按库分摊预算（总量 40 文件 / 单文件 24KB /
      总量 960KB 不变，等分 + 前向借用 + 末尾回收），跨库路径带来源命名空间
      （`{库名}/rel`，重名库 `~2` 消歧），去重键排序后拼接
      （`kb:{id_a}+{id_b}`，防乱序重复建课）。
- [x] **引用归属**：`SourceSpan.kb_id`（蓝图生成后按命名空间标注）；快照披露逐库
      归属（`samples_meta_json.bases`）；课时视图与 t3 读库按 `(kb_id, 库内相对路径)`
      解析；大纲审计的 source 校验语义不变（必须来自已采样路径）。
- [x] **前端**：`CreateCourseDialog` 单选 → 多选（Arco `mode='multiple'`，上限 5）；
      进度/详情/悬浮条"来源：库A、库B"；`CourseWorkspace` 的"回到知识库"按钮指向
      主来源；`LessonSourcePanel` 按 `(kb_id, rel_path)` 解析原文（老课程单库兼容）；
      i18n 双语。
- [x] **agent 工具**：`learning_generate_course` 的 `base` 支持单库名或库名数组
      （候选集仍限会话挂载集）。

**验收：**

- 两库同名文件（都含 `README.md`/`docs/intro.md`）→ 引用与原文回看各自解析到正确库。
- 一库为空 / 一库无 markdown → 明确报错或按库跳过（口径写入票面 Resolution）。
- 删其中一个来源库 → 课程仍可用（快照+其余库），读侧过滤不外泄悬空 id。
- 乱序提交同一组库 → 被去重槽位拒绝（不重复建课）。
- 老课程（单库、裸路径、老快照）零回归。
- `cargo test -p nomifun-learning -p nomifun-db`、`bun run typecheck`、i18n 门通过。

## Resolution

**结论速览（本票拍板）**

- **落库口径**：迁移 **073** 增 `source_kb_ids TEXT NOT NULL DEFAULT '[]'`
  （`json_valid` + `json_type='array'` 校验），老行 `UPDATE … SET source_kb_ids =
  json_array(source_kb_id) WHERE source_kb_id IS NOT NULL` 回填单元素数组（描述流/
  已删库的 NULL 行留空数组）。**`source_kb_id` 列保留不动**：主来源 = 列表首个 =
  该列，不做列退役、不动 `id_schema_contract` 的 SetNull 条目——引用完整性由应用层
  读侧承担（`existing_knowledge_base_ids` 过滤已删库 id，列表/主来源/课时归属都不
  外泄悬空 id）。**课时不新增库归属列**：蓝图 source 写 `kb_id`（落在
  `blueprint_json`），视图 DTO 由采样披露的命名空间映射还原
  `(kb_id, 库内相对路径)`；老课程 `kb_id = None`，前端回退 `course.source_kb_id`。
- **单/多字段优先级**：`knowledge_base_ids` 非空时**列表优先**（按首次出现去重保序，
  首个 = 主来源）；列表为空时回退单 `knowledge_base_id` 的单元素视图（旧客户端逐字
  兼容）。两者同时给出时**单值必须是列表成员**，否则 400（不静默丢来源）。
  上限 `COURSE_MAX_KNOWLEDGE_BASES = 5`（超限 400；按 t2 预算倒推：40 文件等分到
  5 库每库 8 文件）。
- **去重键**：`kb:{id_a}+{id_b}`——**库 id 排序后拼接**（顺序不敏感）；单库保持 t2
  的 `kb:{id}` 形状。乱序提交同一组库命中同一生成槽位（验收钉子③）。
- **分摊常量与算法**：库序 = 库名（trim）+ 库 id 字典序（与提交顺序无关，确定性）；
  逐库配额 = `max(每库保底, 剩余 / 剩余库数)`（前向借用），末尾余量按「打满配额的库」
  均分并二次采样（回收）；每库保底 `MULTI_KB_PER_BASE_MIN_FILES = 4`、
  `MULTI_KB_PER_BASE_MIN_BYTES = 16KB`（上限 5 下等分 8 ≥ 保底，保底是上限放宽时的
  护栏）。总量仍是 40 文件 / 单文件 24KB / 总 960KB；**单库逐字旧行为**（不命名空间化、
  不产逐库披露）。
- **路径命名空间规则**：多库（≥2）采样路径 = `{库名}/rel`；库名清洗（`/`、`\`、
  控制字符 → `_`；清洗后为空或 `.`/`..` → `kb-{库 id 前 8 位}`），同名库按库序加
  `~2`、`~3`…（确定性）。**单库保持裸路径**（零回归）。读侧按披露的
  `bases[].namespace` 去前缀；已删库的路径解析成「无归属」（绝不落到幸存库）。
- **空库口径**：多库下**按库跳过**（该库计数 0、不阻断其他库；内容目录缺失同样跳过），
  仅当**全部**来源库都没有可采 markdown 时才 400；单库缺目录/无 markdown 保持原 400
  （零回归）。
- **披露**：`samples_meta_json` 增逐库条目
  `bases[{kb_id,name,namespace,total_files,sampled_files,coherence_files,low_score_files,goal_fallback}]`
  （有逐库归属才序列化，单库披露 JSON 形状不变）；顶层 `goal_fallback` 仅当所有库
  零命中为真，部分命中时逐库如实标注、`brief_line` 点名回退的库。**无目标的多库路径
  也产披露**（命名空间需要它承载库归属；单库无目标仍无披露，t2 兼容）。
- **归属与读库**：`SourceSpan.kb_id`（serde default，兼容旧 JSON）；蓝图生成后由
  `annotate_blueprint_sources` 按采样命名空间标注；t3 课时实时读库 = 命名空间去前缀
  + `search_bases(&source_kb_ids, …)` 全量来源库（命中自带库归属，主 source 自命中
  按 `(库, 库内路径)` 判等跳过）。
- **前端**：`CreateCourseDialog` 知识库多选（Arco `mode='multiple'`、`maxTagCount`）；
  进度视图/悬浮条/课程详情展示「来源：库A、库B」；「知识来源」按钮指向主来源；
  `LessonSourcePanel` 按 `source.kb_id ?? course.source_kb_id` 读原文；i18n 中英对称。
- **agent 工具**：`base` 收单库名或库名数组（逐名解析、保序去重、上限 5，候选集仍限
  会话挂载集）；`CourseGenerationRequest.kb_id` → `kb_ids`，ai-agent 映射只下发列表
  （主来源由后端从列表派生）。

**验收钉子（测试落点）**

- 两库同名文件 → 引用与原文各自解析到正确库：
  `multi_kb_generation_namespaces_paths_and_resolves_source_owner`、
  `lesson_grounding_reads_the_cited_base_when_two_bases_share_a_path`、
  `multi_base_namespaces_disambiguate_duplicate_names`。
- 一库为空 → 按库跳过：`multi_base_skips_an_empty_base`。
- 删其中一个来源库 → 课程仍可用、不外泄悬空 id：
  `multi_kb_course_survives_a_deleted_source_base`。
- 乱序提交同一组库 → 去重槽位拒绝：
  `multi_kb_shuffled_resubmission_hits_the_same_generation_slot`、
  `multi_kb_key_is_order_insensitive_and_single_compatible`。
- 老课程零回归：`single_base_sampling_stays_unprefixed`、
  `goal_sampling_without_a_goal_falls_back_to_the_legacy_result`、
  t3 老课程用例（无披露/旧快照形状照常生成）。
- 迁移契约：`learning_course_source_kb_ids_migration`（回填 + 数组校验）。
- 命名空间路径的大纲审计：`blueprint_validation_keeps_the_sampled_path_contract_under_namespaces`。

**验证门实跑**

- `cargo test -p nomifun-learning`：208 通过（基线 192，+16 用例）。
- `cargo test -p nomifun-db`：全绿（含迁移 073 契约用例）。
- `cargo test -p nomifun-knowledge`：299 通过 / 1 失败
  （`create_snapshot_source_fetches_and_chains_autogen`，**存量红**：测试断言 blockquote
  头、实现写 YAML frontmatter，与本票无关——本票对该 crate 零改动）。
- `cargo test -p nomi-agent -p nomifun-ai-agent`：nomi-agent learning 工具 15 通过；
  ai-agent 1115 通过（本票新增多库简报文本分支）。
- `cargo check --workspace` 干净；`bun run typecheck` 存量 60 错（videoCanvas 等）
  **未新增**；`bun run gen:i18n` + `check:i18n` 通过；`check:theme`、
  `check:button-layout-contract`、`check:icons`、error/support-surface、
  process-runtime/browser-platform 边界、windows-console-hide 全部通过
  （`check:dead-css` 失败点为存量基线漂移，非本票文件）。

