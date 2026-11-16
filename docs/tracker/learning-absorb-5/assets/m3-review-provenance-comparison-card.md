# m3 评审挂号表 term_provenance 程序化——双臂真机对照卡（开关臂 NOMI_LEARNING_REVIEW_PROVENANCE）

- 日期：2026-10-27；票：m3（#169/#176）
- 二进制：`integr/target/debug/nomicore.exe`，mtime **2026-10-27 17:38:37**，tip `bd7db6188`
- 两臂同构建同凭据（Flowy Cloud flash）、同 fixture：`learning-smoke grow --goal "掌握一元二次方程的解法与判别式应用" --batches 2`，各自全新数据目录

## 两臂命令原文

```
# on 臂（开关 unset）
NOMI_CHANNEL=dev nomicore.exe --data-dir <data-grow-on> learning-smoke grow --db <…>/flowy-backend.db --goal "掌握一元二次方程的解法与判别式应用" --batches 2
# off 臂
NOMI_CHANNEL=dev NOMI_LEARNING_REVIEW_PROVENANCE=off nomicore.exe --data-dir <data-grow-off-prov> learning-smoke grow --db <…>/flowy-backend.db --goal "掌握一元二次方程的解法与判别式应用" --batches 2
```

## 读数行并排（off | on）

| 读数 | off 臂（PROVENANCE=off） | on 臂（缺省 on） |
|---|---|---|
| EXIT | 0 | 0 |
| smoke 尾行 | `OK … batches=2  nodes=10  (grow mode)` | `OK … batches=2  nodes=9  (grow mode)` |
| round 1 挂号读数行 | `[provenance: rows=0 invalid=0 retried=0 degraded=false]`（**行缺席语义 = 零校验**） | `[provenance: rows=21 invalid=0 retried=0 degraded=false]` |
| round 2 挂号读数行 | `[provenance: rows=0 invalid=0 retried=0 degraded=false]` | `[provenance: rows=0 invalid=1 retried=1 degraded=false]`（自然触发死因重试，见下） |
| 存档 `term_provenance` 出现次数 | **0** | **3**（评审 system 契约 + 两轮评审响应挂号表） |
| 评审 findings（存档逐字） | r1 `verdict=pass`（0 findings）；r2 `verdict=reject` 4 条 problems | r1 `verdict=reject` 1 条（「等式」teaches 缺失 → **机械摘除执行**：`review node-removal advice executed mechanically — nodes dropped …["用天平摇摆判断等式两边同加同减后是否仍相等"]`）；r2 首轮 reject 3 条（mints 面）+ 死因重试轮 reject 3 条（assumes 面）→ 保守放行 |
| 节点产出 | 10（两批 5+5） | 9（两批 4+5；r1 摘除 1 节点） |

## on 臂自然触发的死因重试（round 2，逐字）

```
WARN … provenance validation failed — retrying the review once with the death cause attempt=1
  reason=挂号行「方程」的名词未逐字出现在「方程的根」的定义里（常识/缺失行必须来自定义原文）
```
重试后批次仍落地、判据绿（`criteria green`）；读数行如实记 `invalid=1 retried=1`。事实核对：评审驳回意见引用「方程的根」定义原文「代入后能使等式左右两边相等的未知数的值」——确实不含「方程」二字，机械校验**判得对**。✅

## 挂号表来源分布（on 臂 round 1，rows=21）

- `常识` ×20、`缺失` ×1（「等式平衡性」→ term「等式」——该缺失行与评审 problems 里的真缺口互相印证：等式确实无已教出处）。
- r2 重试轮挂号表带 `已教` 行（`{"concept":"一元二次方程的一般形式","term":"未知数","source":"已教","ref":"用字母表示未知量"}`）——ref 解析面在真机在场。

## 人读抽判：常识行可信度（抽 3 行）

| 行 | 抽判 |
|---|---|
| `{"concept":"平方记号 x²","term":"数","source":"常识"}` | ✅ 真常识——「数」是课程外通用概念 |
| `{"concept":"用字母表示未知量","term":"字母","source":"常识"}` | ✅ 真常识 |
| `{"concept":"等式平衡性","term":"左右两边","source":"常识"}` | ✅ 真常识（日常方位词，无需教学） |

3/3 无「把领域名词冒充常识」的误报；同一张表里的 `缺失` 行（等式）被证明是真缺口。

## 如实缺口

- off 臂 r2 评审零挂号表（`term_provenance` 全存档 0 次）而 findings 照常产出——「off = 不要求表、不做机械校验（= k07 后行为）」与实况一致。✅
- >3 轮的重试预算/指纹早停本轮未自然触发（两臂各轮 attempts_used=1）；r2 on 臂的重试后通过走的是「死因重试一次 → 评审契约退化保守放行」链，`degraded=false` 指挂号表退化旗（未两轮不可用），与 `concept review reply violated its contract — degrading to PASS` 的评审退化是两面，读数行如实分开。

## 产物与存档

- on：`rm-matrix/data-grow-on/observation/learning-smoke/run-1793095259231/calls.jsonl`（5 calls，评审 3 次）
- off：`rm-matrix/data-grow-off-prov/observation/learning-smoke/run-1793096045150/calls.jsonl`（4 calls，评审 2 次）
- 日志：`rm-matrix/grow-on-run.log`、`rm-matrix/grow-off-prov-run.log`
