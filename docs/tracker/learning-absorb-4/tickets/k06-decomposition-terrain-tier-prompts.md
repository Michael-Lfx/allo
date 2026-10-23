---
id: k06
title: "分解/地形提示词升级——assumes 逻辑前提判别、块链连续性自检、门语义措辞、tier 深度本位自省"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# k06: 分解/地形提示词升级 + tier 深度本位自省

**What to build:** 吸收 learnhub #149 分解/地形面整改三条提示词条款 + teaches
档位自省的「深度本位」修订（a39d36a）：assumes 只连逻辑前提（判据句）、逐
主干块链连续性自检、硬前置=不可自检豁免非进入闸门的门语义半句、档位自省以
教学深度为准不对照动词字面。全部写作面条款，零机械条款改动，金样随批重生成。

## allo 现状

- 结构提示词资产：`nomifun-learning/prompts/structure/goal-decomposition.md`
  （42 行）+ `prerequisite-terrain.md`（31 行），装配与规格
  `learning_graph/structure_prompts.rs:78-96`；**金样三闸先例就在此 crate**
  （:466 金样重生成 LEARNING_UPDATE_GOLDEN=1、:295 围栏 FENCE_DEV_FACE、
  :388 条款覆盖短语断言）。
- 生长档位概念在位：`ConceptTier`（`learning_graph/mod.rs:57-69`，
  知道<会用<能教）挂在 coach 概念引用（:95 `tier`）；COACH_SYSTEM
  （:374-441）含 est 纪律（:404）与数量契约（:428-430）——档位自省条款
  织入 COACH_SYSTEM 纪律段。
- 结构档 smoke 判据：全灭 0/n 硬判据 + 盘面复验（learning_smoke.rs:1077-1096
  一带）。

## learnhub 参考（#149，提交 50c734a；tier 深度本位修订 = a39d36a）

- `src-tauri/prompts/decomposition/goal-decomposition.md:36` assumes 判据句：
  「assumes 只连**逻辑前提**，判据 = 抽掉这条前置，后继块的定义或推导是否在
  数学上无法进行——练习载体（只拿它练手）与讲解通道（只是讲到的顺手内容）
  不配 assumes，走 contrasts 或不连。」
- 同文件 :37 块链自检：「**块链连续性自检（逐主干）**：每条主干沿 assumes
  链走一遍——后继块的定义需要、而块表中无承接的中间环节，要么补块，要么
  如实写进 gaps，不留悬空环节。」
- `prerequisite-terrain.md:18` 门语义半句：「硬前置的语义 = 不可自检豁免，
  **不是进入闸门**——首门划在真实起步地板处，零基础者进门时本不可能已具备
  地板判据，地面段内容的教学责任在课程，门语义与判据措辞不得写成『须在进入
  课程前具备』一类进门前提。」
- `prompts/node-growth/node-growth.md:42`（a39d36a，用户定调教学质量本位）：
  「档位自省以**教学深度本身**为准，不对照动词字面（动词只是示例，包括但不
  限于）：节点整体是操作训练（如计算/化简/求/作/写出一类）而档位标『知道』
  时，自省是否低估——能按程序做出来，就不只是『知道是什么』；反过来，首教
  形式化这类『写』只是认识步的节点，标『知道』也可能是如实的。」

## 实施要点

- [ ] `goal-decomposition.md` 织入 assumes 判据句 + 块链自检两条（allo 措辞
      对齐，gaps/contrasts 等 allo 契约键位以 allo 契约为准——**先读 allo
      gd.* 契约文档核对键名**，learnhub 键名不照搬）。
- [ ] `prerequisite-terrain.md` 织入门语义半句。
- [ ] COACH_SYSTEM 档位条款换深度本位措辞（适配 allo 三档词汇：操作训练
      节点把 teaches 全标『知道』= 自省低估，能按程序做出来至少『会用』；
      首教形式化『写』只是认识步时『知道』也如实）。
- [ ] **金样随批重生成**（`LEARNING_UPDATE_GOLDEN=1`）+ 围栏 FENCE_DEV_FACE
      过 + 条款覆盖短语断言各资产新增行（:388-461 先例）。
- [ ] 契约文档注记一行（门位口径已 v2.10 对齐过；本批 assumes 判别/块链
      自检注记随批）。

## 验收

- **单测**：三闸全绿（新金样锁定、围栏零违禁、新短语断言命中）；装配面
  无重复段落。
- **smoke structure 档双臂（版本臂，跨构建）**：本票无运行时开关（纯资产
  面）——**旧 tip 构建跑一次 vs 新 tip 构建跑一次**（独立 CARGO_BUILD_DIR、
  禁 stash），同 fixture 目标、同凭据。硬判据：两臂 EXIT 均按既有判据
  （全灭 0/n + 盘面复验）不劣化。
- **对照卡**：双臂结构产物并排读数——assumes 边数、块数、gaps 行数
  （机械读数）+ 人读逐条判：assumes 是否逻辑前提（抽掉后继无法进行）、
  主干链有无悬空环节、门措辞有无「进课前须具备」类进门前提——✅/❌ 两臂
  并排落 Resolution；只跑一臂不算关票。
- `cargo test -p nomifun-learning` 全绿。

## 非目标

- 不建 assumes 逻辑前提/块链连续性的机械校验（learnhub 同为写作面条款；
  机械面留雾区）。
- 不动 STRUCTURE_MOUNT/STRUCTURE_REPAIR 资产（j08/j09 已对账）。
- 不引入 learnhub 的门口梯队 G2..Gn / door_refs 扩入门档（#165 = B 组产品线）。

## 备注

learnhub 载体边病历（carrier-edge-vs-prerequisite，78 块/12 干/103 边）证明
该痛点真实存在——allo 若真机跑出同类载体边误连 assumes，对照卡里如实记录
即为本票价值证据。
