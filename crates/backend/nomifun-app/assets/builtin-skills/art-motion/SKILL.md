---
name: art-motion
description: "Art and motion graphics in code: recreate an animation; paint 35 art styles and make the painting move; 9 explainer grammars (Kurzgesagt/Vox/3b1b/whiteboard/keynote/finance charts); people via AI frames. Use when the user wants art-history motion, explainer clips, or a narration-driven art film. 艺术与视频动画：拆解复刻；代码画风格并让画活；解说动画语法。"
---

# 艺术动画（Flowy 内置）

上游方法来自开源 skill [huashu-art-motion](https://github.com/alchaincyf/huashu-art-motion)（MIT）。Flowy 内置的是**可渲染引擎子集**，不是整个 Git 仓库。许可与裁剪说明见同目录 `NOTICE.md` / `LICENSE`。

你同时是这支片子的导演、分镜师、原画师、动画师和剪辑师，而且你画画用的是代码。成片标准：观众一眼认得出这是哪位艺术家/哪个时代的画，而且画是活的，不是图片在切换。

## Flowy 运行时

本技能只读。开新片时把引擎复制到用户项目，再改副本：

```text
SKILL=.nomi/skills/art-motion          # 或对话工作区里物化后的同名目录
E=<项目>/代码工程
```

Windows 用 `py -3` 或 `python`（不要假设 `python3` / `uv` 一定存在），并设 `PYTHONUTF8=1`。

```bat
set PYTHONUTF8=1
xcopy /E /I "%SKILL%\scripts\engine" "%E%"
py -3 -m pip install playwright numpy Pillow
py -3 -m playwright install chromium
```

若用户环境已有 `uv`，仍可用 `uv run --with playwright python ...`。

出片需要 **ffmpeg** 在 PATH 上。Flowy 桌面端也可能把 ffmpeg 装进数据目录的 `bin/`；命令找不到时先 `where ffmpeg` / `ffmpeg -version`，不要假装已经渲染成功。

**字体：** 内置包不附带 15MB WOFF。缺字时回落到系统字体。要忠实还原风格，从上游 `scripts/engine/lib/fonts/` 把 OFL 字体拷进项目 `代码工程/lib/fonts/`（许可见该目录 `LICENSES.md`）。

**不要**自动配置 Volcengine TTS 或任何付费生图；未选择时只问当前需要的声音/图片方式，可跳过。系统 `say` 在 Windows 上不可用。

长卷 demo 帧、口播整片 reference_films、角色肖像未随包提供。任务需要它们时读 `references/11`、`12`、`10`，用用户素材或 AI 生帧，不要去改内置目录。

## 先判断任务

| 他说的 | 做什么 | 读 |
|---|---|---|
| 「复刻这个动画」「拆一下这段」 | 拆解 → 机制 → 代码复刻 | 01 → 02 → 04 |
| 「做个 XX 风格的动画」「梵高/莫奈/包豪斯那种」 | 一帧先行 → 渲染器 → 让它活 | 03 → 04 → `风格配方/` 找最近的一张当起点 |
| 「用我的口播做一段艺术动画」 | 镜头表 → 定风格 → 世界画布＋镜头 → 画面轨 | 06（草案）＋ 04；口播的转录与时间轴交给你的视频管线 |
| 「配个乐」「卡节奏」 | BPM 网格、动机换乐器、结尾音效序列 | 05 |
| 画面里要出现人（真人、拟人角色） | AI 生帧＋代码合成（B/C 两法） | 10 |
| 派 agent 画某个风格 / 自己写新风格 | 接口、两种起点、硬要求、自检交付 | 08 ＋ `风格配方/INDEX.md` |
| 「做一个人穿过一幅幅名画/一路走过几个世界的片子」 | 长卷骨架：每个世界一个段文件，主角从左走到右，跨边界换画风 | 11（demo 帧未内置，自行准备角色帧） |
| 「做解说/科技/财经视频的动画」「做个 Kurzgesagt/Vox/3b1b/白板/发布会/财经图表那种」、口播管线要一段动画 | 先按口播选语法 → 照语法卡做，库直接调；管线用 `render.py --spec` 出时长精确的片段 | 09 ＋ `动画语法/<语法>.md` |

口播整片先读 `references/12-口播整片与经验回流.md`；讲解员风格再读 `references/动画语法/y6_presenter_explainer.md`。

## 可选口播 / 图片

任务需要新配音时，先读 [语音能力指南](references/capabilities.md)，运行 `py -3 scripts/capabilities.py status --explain`。已有录音直接沿用；无声动画不启动配置。私人音色和凭证保存在安装目录外。

需要图片时读 [图片能力指南](references/images.md)。已有图片直接导入；生成图片按当前会话实际工具和用户许可选择。

## 五层机制（02 号的一句话版）

固定的场景骨架 × 每一幕都是活的画（主角动作＋该风格母题的小循环）× 转场用下一个风格的签名语言 × 节拍网格加速 × 一条连续的叙事锚＋结尾角色梗。换题材照样成立。

## 窄桥：照字面做

- **画面必须动。** 每一幕至少一个主动作＋2个母题循环，自检要量帧差（避开转场后 0.35s 的镜头冲击）。
- **场景与风格用代码画；人用 AI 生帧，代码负责合成。** 场景、风格、运镜、转场、物件用 Canvas 程序化绘制。真人与拟人角色采用已有角色帧，或按画风生成 sprite 帧再抠图合成。几何角色可以用代码画。见 `references/10-角色.md`。
- **复刻是学机制，不是逐像素。** 提取场景、运动与转场规律，再用于新题材。
- **三方向硬门**：新的角色诠释或风格方向，先在同一帧上出三个给他挑，指定了风格也不豁免。
- **确定性**：所有随机用种子，同一时刻渲两次逐像素一致。
- **独立审片**：成片交付前派一个没参与制作的 agent 只看成片挑问题。
- 角色涉及未成年人时，服装一律端庄长款。

## 开阔地：当起点，不当配额

`风格配方/` 里 35 张卡（总表 `风格配方/INDEX.md`）是实做的参数和坑，不是唯一画法。新风格做完，把配方卡写进**项目副本**，不要写回内置 skill。

## 工程

`scripts/engine/` 是可复制的完整渲染工程（引擎、转场库、绘画库、35 个风格场景、渲染器、YouTube 解说语法库、`clip.html`＋`clips/`、示范 JSON `examples/`）。开新片：复制到项目的 `代码工程/`，改 `eras.js` 段落表，写 `scenes/<id>.js`。

```bat
set PYTHONUTF8=1
set E=<项目>\代码工程
py -3 %E%\render.py --solo <id> --stills 0,0.3,0.6 --out 渲染\<id>
py -3 %E%\render.py --out 成片.mp4 --audio 配乐.wav
py -3 %E%\compare.py --a 参考.mp4 --b 成片.mp4 --times 1.5,2.0 --out 对比.jpg
py -3 scripts\analyze\breakdown.py --video 参考.mp4 --out 拆解\
py -3 scripts\engine\render.py --spec scripts\engine\examples\t3_finance_chart.json --out 财经图表.mp4
```

首次渲染前确认 Playwright Chromium 已安装。依赖：Python 3.10+、ffmpeg、Playwright Chromium。

## 验收

交付前必跑 `py -3 scripts/qa.py --project <工程>`（稳定/效率/动感/流畅及文字框景线索）＋派独立 agent 只看成片审片。浅底无字幕成片再用 `py -3 scripts/subzone_gate.py 成片.mp4` 检查图形进入字幕区；纹理底不适用。经验先看 `07-正面经验.md`。

## 回流

每做完一支：新风格写进**项目**里的 `风格配方/` 并更新 INDEX；做对了的事写进项目的 `07-正面经验.md`。不要修改 `{data_dir}/builtin-skills/art-motion/`。
