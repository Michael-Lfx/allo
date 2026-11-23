# 资料库交互示例

> **2026-11-21 当前行为**：生产资料库只展示首页对话经 `publish_file` 明确交付并持久提交的文件，
> 不再提供本地导入或读取 `FLOWY_LIBRARY_DATA`。下文旧版路径引用、注入与重命名说明保留为历史记录，
> 不代表当前产品入口。当前对话文件采用稳定 artifact ID，预览器只接收宿主按 ID 读取的快照字节，
> UI 数据模型不包含磁盘路径。
>
> 本隔离 example 的 `--file` 参数仍可显式读取自有 PDF/PPTX/图片夹具，并经独立 adapter 将字节交给同一预览器；
> 这种输入仅验证预览，不建立对话来源，也不能作为已接入专业文件生成器或持久交付链路的证据。
> 生产预览、另存、默认应用打开、收藏、回收站与来源定位均由应用宿主处理；example 不模拟这些操作已持久成功。
> 本轮新增实现的 Cargo 与原生视觉验收由实施主线程统一记录，不沿用下文旧轮次测试数作为新链路证据。

以下命令均在 `flowy18_wip` 目录执行。此示例用于复现用户提供的资料库截图，并独立体验资料库交互；界面规则沿用现有 `DESIGN.md`。

## 当前对话资料库的隔离截图（2026-11-22）

当前 example 新增以下显式启动参数，均不在生产应用中读取：

| 参数 | 观察状态 |
| --- | --- |
| `--en` | 使用英文；默认简体中文。 |
| `--dark` | 深色主题；默认浅色。 |
| `--empty` | 真空库与「开始对话」入口。 |
| `--search-empty` | 无匹配筛选：空输入记录配合明确的「图片」类型筛选，不伪造检索命中，也不声称已输入查询文本。 |
| `--error` | 空目录读取异常，保留错误反馈，不显示误导的真空库入口。 |
| `--loading` | 资料加载状态。 |
| `--trash` | 将隔离 fixture 标记为回收站条目，观察回收站呈现。 |
| `--file <path> --preview` | 显式读取一份本地预览夹具；读取和路径仅保留在 example adapter 中。 |

`capture-library.ps1` 只启动**已构建**的 example 可执行文件，不调用 Cargo、不访问生产应用数据。它使用
`Start-Process -WindowStyle Hidden` 与原有 `render_to_image` 自动截图开关，依次覆盖中英、亮暗、
800 × 600/1584 × 982、真空库、筛选无匹配、加载、读取异常、回收站、PDF/PPTX 预览。
每个状态生成 PNG、独立 stdout/stderr 日志，最终生成 `capture-report.json`；已有同名 PNG 时拒绝复用，
避免把旧图当成当前结果。超时只停止脚本刚启动的 example 子进程。若当前桌面会话不允许原生窗口初始化，
脚本会保留失败记录，不能把没有截图当成视觉验收通过。

先由实施主线程完成构建，再传入真实可执行文件路径运行：

~~~powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File crates/flowy-studio/examples/capture-library.ps1 `
  -ExampleExecutable 'C:\absolute\path\to\library.exe' `
  -OutputDirectory (Join-Path (Get-Location) '.scratch\conversation-artifacts-native-20261122')
~~~

PNG 应再逐图回读，检查内容、文案、按钮、主题与裁切；脚本成功仅证明原生渲染输出存在，
不能替代首页真实交付、宿主持久恢复、默认应用操作、键盘导航或 UIA 验收。下面旧章节和测试数继续原样保留。

## 运行与截图

```powershell
cargo +1.99 run -p flowy-studio --example library --locked
# 深色主题启动
cargo +1.99 run -p flowy-studio --example library --locked -- --dark
```

- `F6`：切换浅色、深色主题。
- `F8`：导出原生渲染 PNG，默认写入 `.scratch/library-native.png`。
- 设置 `FLOWY_LIBRARY_CAPTURE` 可指定绝对输出路径；设置后启动约 800 ms 自动截图，`F8` 也使用该路径。自动截图前需确保父目录存在。

```powershell
New-Item -ItemType Directory -Force .scratch | Out-Null
$env:FLOWY_LIBRARY_CAPTURE = [IO.Path]::GetFullPath('.scratch/library-native.png')
cargo +1.99 run -p flowy-studio --example library --locked
Remove-Item Env:FLOWY_LIBRARY_CAPTURE
```

示例从 `fixtures/library.json` 加载数据。分类与搜索可组合，排序支持最新、最早和名称，列表与网格可切换；条目菜单提供预览信息、重命名和移除。分类支持方向键、Home、End，菜单与弹窗支持 Escape 关闭。重命名与移除只修改当前进程内存，重启示例后恢复 fixture，不操作真实文件。

应用主体默认空库，不自动加载示例数据。如需在应用主体检查同一批内容，可显式注入 fixture：

```powershell
$env:FLOWY_LIBRARY_DATA = (Resolve-Path 'crates/flowy-studio/examples/fixtures/library.json').Path
cargo +1.99 run -p flowy-desktop --locked
Remove-Item Env:FLOWY_LIBRARY_DATA
```

## 视觉验收

使用用户提供的原始截图作为 reference，不以重绘图代替。安装 Pillow 后运行，替换下面的原图路径：

```powershell
python scripts/compare-library.py --reference "path/to/reference.png" --actual .scratch/library-native.png --output-dir .scratch/library-visual/final
```

脚本将实际截图等比例缩放、左上对齐，输出 `metrics.json`、`overlay.png`、`difference-4x.png` 和内容遮罩；同时报告覆盖率，避免较短截图漏比内容。

本次默认浅色列表验收：原图为 **3168 × 1964**，Windows 150% 缩放下的原生渲染为 **2376 × 1473**，对应同一 **1584 × 982** 逻辑视口。实际图等比例放大 4/3 后完整覆盖原图，得到：

| 指标 | 结果 |
| --- | --- |
| 全图 RGB MAE | 1.897 / 255 |
| 最大通道差异大于 16 的像素比例 | 2.27% |
| 内容遮罩内 RGB MAE | 39.91 / 255 |

大面积空白会降低全图误差，验收仍需结合叠图与内容指标。字体、图标仍有差异，不能据此宣称像素完全相同。原图未展示的网格、菜单、弹窗和深色状态按现有 UI 系统补全。

## 本地文件预览（2026-11-20）

应用资料库提供“添加文件”：通过系统文件选择器多选原文件，保存路径引用到应用数据目录的
library.json。不会复制或更改文件；移动原文件后会提示找不到文件。重复添加同一路径不会增加条目。
正常启动为空库，不再在生产入口回退到演示记录。FLOWY_LIBRARY_DATA 继续作为只读注入来源，
其中相对路径以 JSON 所在目录解析，新增的本地记录独立持久化。两种来源各限 4 MiB 和 1,000 条记录，
启动时在后台读取。资料列表显示本地文件的类型和已知大小。

单击条目进入预览。JPG、PNG、WebP 显示图片；PDF 与 PPTX 支持逐页查看、页码跳转、适应窗口及缩放。
PPTX 是静态预览，布局细节可能与原应用不同。预览窗口及条目菜单提供“用默认应用打开”。
旧版 .ppt 等不支持的格式也可以主动选择该操作；不会自动启动其他应用。

隔离 example 可通过路径直接检查真实文件（添加文件及持久化由应用宿主实现）：

~~~powershell
cargo run -p flowy-studio --example library -- --file crates/flowy-studio/examples/fixtures/preview/sample.pdf --preview
cargo run -p flowy-studio --example library -- --file crates/flowy-studio/examples/fixtures/preview/sample.pptx --preview --dark
~~~

截图仍使用 FLOWY_LIBRARY_CAPTURE。FLOWY_LIBRARY_CAPTURE_DELAY_MS=5000 可为后台渲染预留时间；
FLOWY_LIBRARY_SMALL=1 使用 800 × 600 逻辑视口，FLOWY_LIBRARY_CAPTURE_EXIT=1 在截图后退出该示例进程。
这些开关只用于隔离 example。fixtures/preview 中包含自有测试输入、生成脚本及许可说明。

预览输入限制为 128 MiB。图片解码前检查尺寸，单边最多 16,000 像素，总计最多 1,600 万像素。
PPTX 最多 4,096 个 ZIP 条目，单条目最多展开到 32 MiB，总展开上限为 128 MiB。
单页光栅化最多 1,600 万像素，较大页面降低渲染分辨率。先渲染当前页，再按剩余预算逐页预取相邻页。
当前渲染的输出与已缓存位图共用 128 MiB 预算，关闭时释放纹理。内容编辑、动画、全文搜索和密码输入不在首版范围。

本地预览本轮验证（2026-11-20，Windows）：flowy-app 37 项、flowy-i18n 5 项、flowy-studio 39 项，
共 81 项测试通过；桌面端 check/build、UI contracts、本次变更文件的格式与空白检查通过。
原生截图已检查浅色图片/PDF、深色中文 PPTX 和 800 × 600 小窗口。
全仓格式检查与 Clippy 仍存在既有代码问题，本轮没有修改这些无关文件。

## 验证与构建限制

```powershell
cargo +1.99 test -p flowy-ui -p flowy-i18n -p flowy-studio -p flowy-app --lib --offline --locked
```

本次共 52 项测试通过：`flowy-ui` 18、`flowy-i18n` 4、`flowy-studio` 12（含资料库 7 项）、`flowy-app` 18。桌面端 check/build 已通过；UI 与 Studio 的 `clippy --all-targets --no-deps` 已通过，仍有既有警告。

构建仍依赖本机 `.scratch` 中的 GPUI patch，未跟踪补丁的可移植性问题尚未解决。
