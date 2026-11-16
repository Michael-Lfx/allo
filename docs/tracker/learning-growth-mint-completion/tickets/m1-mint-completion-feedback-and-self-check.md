# m1: 铸名缺漏——门打回反馈升级为可照抄补丁 + 起草合同铸名完备性自查

## Status

施工收口，真机冒烟留观（2026-11-16 开票即关；冒烟读数回填 Resolution）。

## 已核验事实（2026-11-16 三面对照）

- **死因链（坐实）**：新课 01a1e364 首批 draft teaches 引用「共享状态」
  （结构参考块名同名词），mints 只有「短路求值」；结构门
  `ng.refs.resolvable` 打回；两轮重裁产物同指纹早停，3 次调用 75s。
- **为何只有它被打回**：值分类/函数定义/函数调用/语法错误识别 在全局
  登记表有行（旧课铸名，概念不灭删课不清表）；共享状态从未被铸名。
- **重裁为何没修上**：通用反馈（「先铸名或改用登记表中的名字」）无可
  照抄形状；第一次重裁越权改未点名节点被守卫拦（原地重试），第二次被
  splice 拼回，指纹同首轮。
- **模型混杂因子（如实记录）**：本次为手动 grow 且用户选了
  AIPC-deepseek-v4.1-flash（自动臂恒 default）；旧课 v4-flash 同型场景
  铸名成功。b1 修复生效（prompt_chars 12786→14098、draft note 引用声明
  概念为已教账）与换模型两因子混杂，行为变化无法干净归因。
- **系统性陷阱**：结构块名长得像概念名；未声明块的概念铸名前永不在
  登记表——「照块名引用→漏铸名」任何模型可踩。

## Terms

- **R1（门打回反馈升级）**：`growth_retry::mint_completion_feedback`——
  门红条 `ng.refs.resolvable` 无宿主臂逐名抽取，附可照抄的 mints 补条
  形状（canonical=该名/aliases=[]/definition 按节点 scope 概括/
  source=教练提名）；多候选臂排除（已有逐字照抄指引）。只追加进
  feedback，不进指纹（指纹 = outcome.errors JSON）、不进守卫点名面
  （补丁无节点标题）、不进评审。
- **R2（起草合同自查句）**：COACH_SYSTEM【起点纪律】加「提交自查（铸名
  完备性）」bullet——teaches/assumes 每个名字必须在 已教账面/登记表
  切片/本批 mints 三者其一；**结构参考材料里的块名不是登记表名**；拿
  不准宁补 mints（铸名≠教）。无条件进基线（缺陷指引类；用户已裁决不
  走开关臂）。
- **R3（不做）**：机械自愈（自动补 mint + 合成定义落登记表）不做，复发
  再立案；门/守卫/早停机制零改动；已删课程的登记表残留行不动（概念
  不灭设计，resolvable≠供给）。

## Acceptance

- `cargo test -p nomifun-learning` 全绿（新增
  `mint_completion_feedback_guides_the_exact_patch`：无宿主臂逐名补条、
  多候选臂/其它 kind 排除、空输入零附加；
  `coach_system_carries_the_mint_completeness_self_check` 三短语在位）。
- 逐字节臂对照测试（anti_loop boundary_dedup）不受影响（条款无条件进
  两臂，replace 后仍相等）——已验证。
- 真机冒烟（留观）：复制活库快照 → 同模型（v4.1-flash）重放新课 grow
  → 批次通过落库；teaches 不重教已声明概念；共享状态（或同类块名概念）
  随批铸名在册；读数回填本节。

## Resolution

已施工收口（2026-11-16，feat/learn1116，a9de629f6）。R1/R2 落地，测试
967 绿。**真机冒烟通过**（2026-11-16 17:00，快照重放）：

- 配置 = 活库 VACUUM INTO 快照 + 同目标 + **同失败模型**
  （AIPC-deepseek-v4.1-flash）+ `learning-smoke loop --rounds 1`（结构 +
  首批）。新课继承全局声明账面（45 概念）+ 全局登记表（旧课铸名行），
  陷阱压力与失败跑同构。
- 结果：**draft 一次过门零打回零重裁**——教练随批正确铸名 变量绑定/
  短路求值（带定义与出处），值分类/函数定义/语法错误识别 走登记表既有
  行不重复铸名；概念评审一次过，批落库 5 节点。
- 质量面：teaches ∩ 声明45 = **∅**（不重教已声明概念）；assumes 全部
  落在声明集内（对声明者合法）；节点 intent/scope 边界完整（抽查
  短路求值：不教三值逻辑/惰性求值/并发内存可见性，difficulty 2 est 20）。
- 冒烟 EXIT=2 如实说明：违规行全部来自 smoke 审计器的供给模型**不含
  声明**（按既往落库 teaches 算「既往供给」）——smoke 桩 limitation，
  非产品缺陷；growth 本体 applied。审计器是否补声明口径留后续候选。
- 单样本如实声明：一次过门 = 自查条款生效或抽样运气，二者不可分；
  门打回侧的可照抄补丁（R1）在再触发时才受真实考验。
