# 首页动态背景性能踩坑与交接

> 2026-11-11 · `feat/home-canvas2d` · 代码、前端检查和 Chrome 组件对照已验证。实际卡顿设备上的 WebView2 性能待测。
>
> 接手先采集目标设备的输入、切层与 GPU 数据，再决定是否继续优化。
>
> 2026-11-11 晚更新：三处创作背景已从 Canvas 2D 改为全屏静态 CSS 光氛围层（`.glow` / `.grain` / `.veil`，零逐帧绘制）；随后升级为「单层 conic 光锥 + 指针视差」版本（见下节），下方 Canvas 2D 数据保留为历史对照。

## 当前实现：单层光锥 + 指针视差（2026-11-11 晚）

- 常驻动画只有一层：`.rays` 的 conic-gradient 光锥，90 秒旋转；创作=紫罗兰、图片=暖琥珀、视频=冷蓝，几何与色相由 CSS 变量按变体驱动。
- `.glow` 静态光池随指针平移（最大 ±10px），由单例 rAF 更新两个 CSS 变量；指针静止或离开后循环停止。`prefers-reduced-motion` 下不挂监听、光锥动画关闭；`data-paused` 在视频预览播放时暂停光锥；`forced-colors` 隐藏装饰。
- 浅色主题下光锥改用 `multiply` 混合，透明度降低，避免 screen 在浅底上泛白。
- 实测（16 核本机，CDP Performance + GPU 进程 CPU，2026-11-11）：常驻光锥相对关闭时 GPU 进程 CPU 增加约 **0.6–0.8 个百分点**（整机口径），每 10 秒约 1,060–1,109 次样式重算；Paint 事件 0–2 次，Raster 次数无显著变化；偶发 1–3 帧超过 25 ms，未见持续掉帧。证据与原始 JSON/截图/录屏：`build.noindex/ambient-checkpoint/evidence/`。

## 当时的 Canvas 2D 实现与可比基线（历史对照）

首页创作目录、图片、视频共用 `CreateAmbientWall`，分别绘制波纹、点阵和时间轴刻度。每帧清理一次小画布，提交一次描边。显示区域最大 **720×112 CSS 像素**，缓冲区上限 **48,000 像素**，调度上限 **10 FPS**。背景使用当前主题的底色与强调色，播放视频预览时暂停，系统减少动态效果开启时保留静止画面。 [首页挂载分支](../../ui/src/renderer/pages/guid/GuidPage.tsx) · [Canvas 2D 图案](../../ui/src/renderer/pages/guid/components/CreateAmbientWall.tsx) · [画布尺寸](../../ui/src/renderer/pages/guid/components/CreateAmbientWall.module.css)

| 版本 | 背景实现 | 如何识别 |
| --- | --- | --- |
| `fcdc89c13` | Paper Shaders 的 `GodRays` | 创作背景调用 `getContext('webgl2')` |
| `3228c2b18` | Canvas 2D 柔光贴图 | 每帧缩放并混合两张透明贴图，预算为 30 万像素、15 FPS |
| 本文所在版本 | 局部 Canvas 2D 动态图案 | 每帧一次 `stroke()`，三处组件仅请求 `2d` |
| 当前版本（2026-11-11 晚） | 全屏静态 CSS 光氛围层 | 三处组件不再创建 canvas，只有 `.glow` / `.grain` / `.veil` 三个静态层 |

如果客户端仍显示神光，先确认正在运行的 checkout 和前端产物对应哪个版本。共享侧栏、终端、作品球面墙和视频画布可有各自的 WebGL 用途，分析整个客户端时按调用来源归因。 [共享侧栏渲染](../../ui/src/renderer/components/base/GooeyNav.tsx) · [终端渲染](../../ui/src/renderer/pages/terminal/XtermView.tsx) · [视频作品球面墙](../../ui/src/renderer/pages/videoGeneration/components/InfiniteMenu.tsx)

## WebGL 与 Canvas 2D 性能踩坑

- **Shader 的 `speed` 调的是时间推进。** 旧版 GodRays 的低档用了非零的 `0.25` 和 `0.35`，仍持续排帧。在锁定的 Paper Shaders 0.0.82 渲染循环上模拟 60 Hz，`speed=0.25` 和 `0.55` 都是 60 次绘制/秒。单纯能创建 WebGL2 上下文也无法识别低性能和软件渲染路径。
- **预绘贴图仍有合成成本。** 2D 柔光版复用了小贴图，每帧把它们拉伸为大面积半透明图层并混合。常规 1280×800 视口里，Canvas 的 CSS 盒子仍铺满 1280 像素宽。当前图案同时限制缓冲像素和显示面积，去掉创作三层的整屏透明度分组。 [背景外层样式](../../ui/src/renderer/pages/guid/index.module.css)
- **分别看画布 API、光栅化和合成。** Chrome 对照时报告 GPU 合成已开启，组件只请求 Canvas 2D。下表的清理像素和调用数描述绘制量。整机显卡占用、驱动、合成和输入延迟需要目标设备的 trace。
- **避免给动效帧加像素读回。** 当前主题采色使用单独的 1×1 Canvas，只在主题样式失效后读取。性能采样可记录调用次数和缓冲尺寸。逐帧 `getImageData()` 会引入额外工作，影响比较。 [主题采色](../../ui/src/renderer/pages/guid/components/ambientCanvas.ts)

### Chrome 组件对照

同一个 1280×800 视口，每项取约一秒稳定态。旧版为 `3228c2b18` 的 Canvas 2D 柔光版。新旧样本的上下文请求均为 `2d`。清理像素为该样本内 `clearRect` 覆盖的缓冲像素之和。

| 入口 | CSS 显示区，旧 → 新 | 缓冲像素，旧 → 新 | 清理像素，旧 → 新 | 贴图调用，旧 → 新 | 新版描边次数 |
| --- | --- | ---: | ---: | ---: | ---: |
| 创作目录 | 1280×464 → 720×112 | 299,061 → 47,730 | 4,186,854 → 429,570 | 28 → 0 | 9 |
| 图片 | 1280×464 → 720×112 | 299,061 → 47,730 | 4,186,854 → 477,300 | 28 → 0 | 10 |
| 视频 | 1280×384 → 720×112 | 300,000 → 47,730 | 4,200,000 → 477,300 | 28 → 0 | 10 |

清理像素下降约 **89%**。同一组件测试还验证了暂停时切换壁纸协调色只补绘一帧，以及系统减少动态效果开启后的空闲绘制为零。

## 调度、缩放和主题踩坑

| 触发场景 | 确认的问题 | 当前处理 |
| --- | --- | --- |
| 连续改变窗口或画布布局尺寸 | 写 Canvas 宽高会清空缓冲区。观察器先写宽高、绘制等下一个节拍，会在拖拽期间看到空画布。 | 观察器只标记尺寸失效。在允许绘制的回调内一起调整缓冲、设置变换、出图。宽高变化时先缩小旧维度，避免中间缓冲超预算。Chrome 连续改变 60 次 CSS 尺寸的样本中空白帧为 0。 |
| 同尺寸恢复 Canvas 上下文 | 绘制变换和缓冲会复位。仅靠尺寸变化更新，恢复后可能裁切。暂停时还可能一直空白。 | 丢失时停止调度。恢复时重设变换、使绘制缓存失效并补绘。模拟暂停恢复确认只绘制一帧。 |
| 高刷新率或帧时间抖动 | 单独限制绘制次数仍会每刷新一次就请求回调。RAF 的共享时间戳还可能早于定时器实际触发时间。 | 在接近下一绘制期限时申请 RAF，以 `performance.now()` 判定到期。修复过程中，15 FPS 档的 Chrome 样本曾出现 19 次回调、15 次出图，修正后样本为 14 次回调、14 次出图。 |
| 深浅主题、同档换肤和壁纸协调色 | 只观察明暗属性会漏掉样式表更新。深色默认选择器的优先级也可能盖过壁纸的通用主色。 | 监听相关样式表与主题属性，按解析后的颜色更新绘制。壁纸协调色使用独立的 `--wallpaper-accent`，关闭后从当前主题取色。 |

[共享调度实现](../../ui/src/renderer/pages/guid/components/ambientCanvas.ts) · [调度回归测试](../../ui/src/renderer/pages/guid/components/ambientCanvas.test.ts) · [壁纸色词生成](../../ui/src/renderer/utils/theme/wallpaperTokens.ts) · [HTML 渲染和 Canvas 恢复顺序](https://html.spec.whatwg.org/multipage/webappapis.html#update-the-rendering)

## 接手复测与优化判断

1. 在用户反馈卡顿的 Windows WebView2 上固定 Runtime 版本、窗口大小、系统缩放、刷新率、主题、侧栏状态和壁纸设置。记录设备实际使用的 GPU 或软件渲染状态。用相同的 DevTools Performance 配置采样，并记录对应的 WebView2 进程指标。 [Microsoft WebView2 性能诊断](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance)
2. 对照 `3228c2b18` 柔光版、当前图案版和临时绕过 `CreateAmbientWall` 挂载的诊断版。每个入口先录 20 秒前台空闲，再各做 30 次进入、返回和键入。记录各组的 Renderer CPU、GPU 进程占用、掉帧，以及输入或点击到下一次可见更新的 P95。
3. 核对视频预览播放与暂停、减少动态效果、深浅与同模式换肤、窗口缩放。普通模式下三个图案应仍可见且有动态，暂停期间应没有持续背景绘制。录制 GPU/交互 trace 时关闭下方的操作计数探针。
4. 如果当前版相对无背景对照的输入和切层 P95 差值在约一个刷新周期内，且 CPU/GPU 差异没有稳定复现，可把当前版作为基线。60 Hz 的一个刷新周期约为 16.7 ms。如果仍有明显差距，用 trace 先区分背景绘制、合成、视频解码与共享侧栏，再决定是否缩小对应区域或减少绘制。保留用户要求的 Canvas 2D 动态。

### 在当前版本计数

进入创作目录、图片或视频后，在 WebView2 DevTools Console 运行。固定窗口，等待约一秒打印样本。探针会还原临时包装的方法。

```js
(() => {
  const canvas = document.querySelector('[data-testid="guid-create-ambient"] canvas')
  if (!canvas) throw new Error('先进入一个创作背景')
  const context = canvas.getContext('2d')
  if (!context) throw new Error('本探针适用于已挂载的 Canvas 2D 背景')

  const calls = { clearRect: 0, drawImage: 0, stroke: 0 }
  const methods = Object.keys(calls)
  const previous = methods.map(method => Object.getOwnPropertyDescriptor(context, method))
  const startedAt = performance.now()

  for (const method of methods) {
    const original = context[method]
    context[method] = function (...args) {
      calls[method] += 1
      return original.apply(this, args)
    }
  }

  setTimeout(() => {
    for (const [index, method] of methods.entries()) {
      const descriptor = previous[index]
      if (descriptor) Object.defineProperty(context, method, descriptor)
      else delete context[method]
    }
    const { width, height } = canvas.getBoundingClientRect()
    console.table({
      durationMs: Math.round(performance.now() - startedAt),
      cssWidth: width,
      cssHeight: height,
      backingPixels: canvas.width * canvas.height,
      ...calls,
    })
  }, 1000)
})()
```

### 前端验证

本轮通过 187 项首页和壁纸相关测试、TypeScript 检查、主题/CSS scope/dead CSS 检查与前端生产构建。常用复跑命令如下。

```bash
bun test ui/src/renderer/pages/guid ui/src/renderer/utils/theme/wallpaperTokens.test.ts
bun run typecheck
bun run check:theme
bun run check:css-scope
bun run check:dead-css
bun run build:ui
git diff --check
```

待验收项为实际卡顿设备的 WebView2 交互与 GPU 轨迹。Chrome 组件证据可用于复核绘制量和边界行为。
