# 资料库交互示例

以下命令均在 `flowy18_wip` 目录执行。此示例用于复现用户提供的资料库截图，并独立体验资料库交互；界面规则沿用现有 `DESIGN.md`。

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

## 验证与构建限制

```powershell
cargo +1.99 test -p flowy-ui -p flowy-i18n -p flowy-studio -p flowy-app --lib --offline --locked
```

本次共 52 项测试通过：`flowy-ui` 18、`flowy-i18n` 4、`flowy-studio` 12（含资料库 7 项）、`flowy-app` 18。桌面端 check/build 已通过；UI 与 Studio 的 `clippy --all-targets --no-deps` 已通过，仍有既有警告。

构建仍依赖本机 `.scratch` 中的 GPUI patch，未跟踪补丁的可移植性问题尚未解决。
