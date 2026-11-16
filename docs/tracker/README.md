# 本地 tracker（wayfinder 本地 md 约定）

仓库的正式 issue tracker 是 GitHub Issues（见 `docs/agents/issue-tracker.md`）。**wayfinder
地图与票按用户要求落在本地 md 文件**，存放于本目录，每个 map 一个子目录：

```
docs/tracker/<effort-slug>/
  map.md                # 地图（wayfinder:map）
  tickets/<id>-<slug>.md # 每票一文件
  assets/               # 票结案产出的研究报告等资产
```

## 票文件格式

```markdown
---
id: g1                          # 稳定 id，文件名与 blocked_by 引用它
title: <票名>
type: grilling                  # wayfinder:<type> = research | prototype | grilling | task
status: open                    # open | closed
assignee:                       # 认领人；认领 = 会话开工前第一个写操作
blocked_by: [g2]                # 阻塞者 id 列表；全部 closed 才解锁（前沿 = open、无阻塞、未认领）
part_of: map.md
---

## Question
<本票解决的决策或调查>

## Resolution                  # 结案时追加；首部必须是结论速览
<结论 + 指向 assets/ 的链接>
```

## 操作约定

- **认领**：开工前把 `assignee` 写成自己，会话结束未解决则清空。
- **解决**：写 `## Resolution`、`status: closed`，并把一行摘要（票名 + 链接 + 一句话结论）追加进
  `map.md` 的 Decisions so far。
- **阻塞**：只在 frontmatter `blocked_by` 里表达；新建票先落文件再回填引用（create-then-wire）。
- **雾毕业**：`map.md` 的 Not yet specified 里能说清问题了，就建票并从雾里删掉该条。
- research 票结案评论首部放结论速览（表格或要点），完整报告存 `assets/`。
