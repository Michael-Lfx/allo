# PPTX / PDF / JPG 预览依赖调研

> 调研日期：2026-11-20。范围：当前 Flowy GPUI workspace、GPUI CE Web 后端，以及普通浏览器预览。

## 建议

| 格式 | Rust / 原生 GPUI | 浏览器 | 备注 |
| --- | --- | --- | --- |
| JPG | 直接用 GPUI `img` / `Image`，无需新增解码依赖 | 原生 `<img>` / Blob URL | 本仓库 GPUI CE 默认启用 `image-default-formats`，包含 JPEG；`cargo tree -e features -i image@0.25.10` 也确认当前依赖图启用了 `image/jpeg`。需要改像素或编码时再显式依赖 `image`。 |
| PDF | 先 POC `hayro`；若真实文件兼容性不够，改用 `pdfium-render` 并随包提供匹配平台的 PDFium | `pdfjs-dist` | Hayro 纯 Rust、CPU 位图渲染，但文档列出未优化性能及加密 PDF 等限制；PDFium 更成熟，但绑定本身不包含 PDFium 动态库。 |
| PPTX | 先 POC `rpptx = "0.15.0"`，直接导出页 PNG 或 PDF；对照不通过时，以 LibreOffice headless 转 PDF 作为兼容性兜底 | 转成 PDF 后交给 PDF.js | `rpptx` 是近期发布的 Rust 原生方案，渲染 feature 默认启用；它不是 PowerPoint 本身，必须用项目真实文件核对输出。LibreOffice 路线成熟，但要管理外部程序、字体和跨平台打包。 |

不要把 `gpui-pdf 0.8.2` 直接加到当前 workspace：它依赖 `gpui-pre ^0.3.12`，而本项目用 `gpui-ce 0.3.0`；两者会形成不同 GPUI 类型图。它的分页虚拟化、后台渲染、搜索/高亮、表单及密码提示值得借鉴，但应先确认 API / 类型兼容，或把 viewer 渲染层移植到 GPUI CE。

## 按运行环境选型

### 原生 Rust + GPUI CE

- **JPG：**现有 `gpui-ce 0.3.0` 默认打开 `image-default-formats`，该 feature 转启 `image/default-formats`，其中包含 JPEG。直接将文件字节/路径交给 GPUI 图片元素即可；不要只为显示图片再增加一份解码器。若未来关闭 GPUI 默认 features，需重新确认 JPEG feature 是否仍由 workspace 其他依赖启用。
- **PDF：**Hayro 将 PDF 页渲染为 bitmap，适合本地页预览；需自行处理缩放、滚动、缩略图缓存和内存上限。它当前文档明确说明渲染仍在实现缺失功能、尚未做性能优化，并列出密码保护 PDF、blending/isolation 等限制。若兼容性优先，`pdfium-render` 有更广泛的 PDFium 能力，但它只是绑定层：发布包需按 Windows/macOS/Linux 与 CPU 架构携带 PDFium。
- **PPTX：**`rpptx 0.15.0` 有 `Presentation::slide_png_deterministic()` / `slide_pngs_deterministic()` 和 `to_pdf_deterministic()`，Rust 原生输出可直接进入 GPUI 图片页或复用 PDF viewer。默认 feature 包含 `render`。截至调研日，它是 docs.rs 当前版本，2026-11-09 发布；采用前应针对图表、SmartArt、主题字体、母版/占位符、渐变/阴影、嵌入媒体和动画准备样本，检查 renderer 返回的诊断，再判断是否可代替 Office 渲染。
- **LibreOffice 兜底：**用 `soffice --headless --convert-to pdf --outdir <cache-dir> <deck.pptx>`，生成 PDF 后共用 PDF 页面渲染器。转换放后台线程/进程，缓存按输入内容失效；要在目标 OS 上决定打包 LO，还是把它作为可选外部程序。

可先在实际调用 crate 中显式声明：

```toml
rpptx = "0.15.0"
hayro = "0.9"
```

其中 `rpptx` 默认启用 `render`；`hayro` 当前 API 可渲染到像素图。若需要自行将像素数据编码为 PNG，再按需要显式增加 `image` 并只打开 `png` feature。先在承担预览功能的 app crate 添加依赖，不要放入通用 `flowy-ui`。

### Web 与 GPUI Web

- **普通 Web / WebView 页面：**JPG 用浏览器原生 `<img>`；PDF 用 Mozilla PDF.js 的 `pdfjs-dist`，按官方例子配置 worker 并逐页绘到 canvas。
- **PPTX：**优先由本地 Rust renderer 或服务器转换为 PDF，再交给同一份 PDF.js viewer，降低两套页控件和缓存逻辑。若需求禁止转换服务且必须浏览器本地打开 PPTX，可另行 POC 浏览器 PPTX renderer；这类方案不能自动获得 PowerPoint 的完整排版一致性。
- **GPUI Web：**`gpui_ce_web 0.3.0` 是独立 WASM 后端，源码描述其使用一个由 document 持有的 canvas、一个顶层窗口；普通 PDF.js viewer 的 DOM 不会自然成为 GPUI 元素。要复用 PDF.js，需把 offscreen canvas 的像素/位图交给 GPUI Web 渲染，额外评估 worker、拷贝成本和 WebGPU 纹理生命周期。当前仓库直接依赖是桌面 GPUI CE，尚未将 `gpui_ce_web` 接入 Flowy app；这是额外平台工作，不只是加一个 crate。

## 结论和落地顺序

建议先建独立 preview service 边界：输入文件字节和格式，异步产出按页的尺寸 + 位图/错误；UI 负责虚拟列表、缩放和缓存。先做 JPG（零新依赖），再用真实 PDF 集评估 Hayro；若不满足兼容性/性能，改 PDFium 并安排动态库打包。PPTX 用 `rpptx` 跑可视差异样本，未过门槛时走 LibreOffice 转 PDF。Web 端通过 PDF.js 消费同一种分页输出策略；GPUI Web 再单独做一次像素桥接验证。

## 一手资料

- [GPUI CE 0.3.0 feature flags](https://docs.rs/crate/gpui-ce/0.3.0/features)：默认启用 `image-default-formats`。
- [`image` 0.25.10 formats/features](https://docs.rs/crate/image/0.25.10/features)：`default-formats` 含 `jpeg`。
- [Hayro 文档](https://docs.rs/hayro/latest/hayro/)：CPU 位图 renderer、当前性能/功能限制。
- [`pdfium-render` binding 文档](https://docs.rs/pdfium-render/latest/pdfium_render/)：绑定 PDFium、运行时动态库选项。
- [`gpui-pdf` crate](https://docs.rs/crate/gpui-pdf/latest)：viewer 能力及其当前 `gpui-pre` / `hayro` 依赖。
- [`rpptx` 0.15.0 crate](https://docs.rs/crate/rpptx/0.15.0) 与 [`Presentation` API](https://docs.rs/rpptx/0.15.0/rpptx/struct.Presentation.html)：PPTX 读取、PNG/PDF 导出、feature 与版本。
- [LibreOffice start parameters](https://help.libreoffice.org/latest/en-US/text/shared/guide/start_parameters.html)：headless 和命令行参数；[PDF export](https://help.libreoffice.org/latest/en-US/text/shared/01/ref_pdf_export.html)。
- [PDF.js Getting Started](https://mozilla.github.io/pdf.js/getting_started/) 与 [canvas rendering examples](https://mozilla.github.io/pdf.js/examples/)；[许可证](https://github.com/mozilla/pdf.js/blob/master/LICENSE)。
- [`gpui_ce_web` 0.3.0 源码](https://docs.rs/crate/gpui_ce_web/0.3.0/source/src/gpui_web.rs)：浏览器后端和单 canvas 说明。
