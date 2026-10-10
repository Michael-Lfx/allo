# Wave 1：声学底座与模型管理 (Acoustic Foundation & Model Management)

> **对齐**：基线 Phase 0 + Phase 1（两阶段合并执行，出口仍分 Gate 0、Gate 1 两次评审）
> **前置**：Gate S 通过；契约 C1-C8 冻结
> **出口门禁**：Gate 0（管线重构 + 状态机单测）→ Gate 1（召回/误唤醒/RTF/RSS + 模型双场景链路）

---

## 1. 目标与范围

把 Wave 0 的哑声学链替换为真实生产实现：无锁 SPSC 管线、KWS 唤醒、VAD 切分、SenseVoice ASR；同时交付 `ModelManager` 完整生命周期（真实双源下载、SHA256 强校验、REST/SSE、三场景分发）。本波结束时设备能以 CLI 形态完成“唤醒 → 转录”。

## 2. 任务卡

| 任务卡 | 属主 | 产出 | 完成定义 (DoD) | 映射 |
| :--- | :--- | :--- | :--- | :--- |
| T1-01 | WS-A | `mic.rs` 重构：CPAL 回调零锁零分配，预分配 SPSC ringbuf；重采样/声道折叠移入 worker 线程；`read_chunk` 改用 C8 枚举 | VT-1-01 绿；10 分钟采集 0 阻塞 0 内存增长 0 xrun | Gate 0 |
| T1-02 | WS-A | KWS 接入：固定模型版本，`keywords_source.txt` 经 `text2token` 编译 `keywords.txt` | 编译链脚本可重复执行 | Gate 1 |
| T1-03 | WS-A | 三口音语料校准定标（阈值从语料集得出，非拍脑袋） | 校准报告归档；VT-1-02/03 绿 | TC-AM-001/002 |
| T1-04 | WS-A | Silero-VAD 切分 + SenseVoice ASR 接通（复用 `stt/sherpa.rs`） | VT-1-04 绿；RTF < 0.25 | Gate 1 |
| T1-05 | WS-C | 双源探针下载器：HEAD 1500ms 并发探测、首包 <800ms 选源、Range 断点续传、官方 CDN 兜底 | VT-1-05/06 绿（对 fixtures 本地源） | TC-AM-016 |
| T1-06 | WS-C | SHA256 强校验：内嵌权威清单比对、Corrupted 拦截、自检通过零网络请求 | VT-1-07/08 绿 | Gate 1 |
| T1-07 | WS-C | REST/SSE 全量实现：install 幂等、progress 推送百分比与实时速度、delete 安全清理 | VT-1-09 绿 | TC-AM-014 |
| T1-08 | WS-E | `auto_download = true` 开机自检触发自愈下载（宿主侧接线） | VT-1-10 绿 | TC-AM-015 |
| T1-09 | WS-F | 设置行接入真实 status/SSE：四态可视（Probing/Downloading/Verifying/Failed），行内重试 | bsk-1-01 通过 | TC-AM-017（部分，Wave 2 完成全态） |

**执行顺序提示**：T1-01 是声学链的地基，优先于 T1-02~04；T1-05/06 是 T1-07/08 的前置；T1-09 依赖 T1-07 的 SSE 契约（已在 M0 冻结，可提前并行开发，对 fixtures 联调）。

## 3. 纵向测试集 VT-1（先写成失败测试）

| 编号 | 竖切 | 断言 | 通过标准 |
| :--- | :--- | :--- | :--- |
| VT-1-01 | 10 分钟连续回放 → SPSC 管线 | 0 阻塞、0 内存增长、0 xrun 报警 | 确定性通过 |
| VT-1-02 | `corpus/wake` 全量回放 → KWS | 唤醒触发；唤醒词尾音 → wake.wav ≤ 200ms | 召回率 ≥ 95% |
| VT-1-03 | `corpus/noise` 2 小时加速回放 → KWS | 误唤醒次数 | 严格 = 0 |
| VT-1-04 | 指令语料 → VAD → ASR | 转录文本匹配期望；RTF | 匹配率 100%；RTF < 0.25；RSS < 180MB |
| VT-1-05 | `fixtures/http` 限速源 → 下载器 | 探针选源正确；Range 断点续传完成 | 字节级一致 |
| VT-1-06 | 下载中断流 → 自动回退 CDN 源 | 切换发生且最终完整 | 校验通过 |
| VT-1-07 | 完整 fixtures 模型 → 校验 | `Ready`；0 字节外网请求（断网执行） | 哈希 100% 匹配 |
| VT-1-08 | 故意损坏文件 → 校验 | `Failed`/`Corrupted`；不触碰 ONNX Runtime | 无崩溃无加载 |
| VT-1-09 | install → SSE 流 → delete | 事件序列含百分比与速度；删除后回 `NotInstalled` | golden 事件序列匹配 |
| VT-1-10 | 空目录 + `auto_download=true` 开机 | 自动进 `ProbingSource` 并完成安装 | 全流程无人工干预 |

## 4. bsk 验证剧本

| 剧本 | 步骤 | 断言 |
| :--- | :--- | :--- |
| bsk-1-01 设置行实时进度 | 后端指探针到 fixtures 本地源 → bsk 打开设置行 → 点【下载并启用】 | 发丝轨道 `scaleX` 推进（computed style 无 width 动画）；百分比/速率文本 tabular-nums；阶段文案随 SSE 更新；行高恒定 |

## 5. 出口门禁检查表

- [ ] Gate 0：`cargo test -p nomifun-audio` 全绿；VT-1-01 绿；`ModelInstallState` 单测绿
- [ ] Gate 1：VT-1-02~10 全绿；召回 ≥95%；2h 误唤醒 0；RTF < 0.25；RSS < 180MB；离线拷贝 0 网络请求
- [ ] `cargo fmt -p` 已跑；bsk-1-01 记录归档

## 6. 本波禁止事项（追加）

- 禁止为赶召回率在 KWS 通路外加任何“补丁式”声学后处理（阈值问题回到语料校准 T1-03 解决）。
- 禁止把 fixtures 本地源 URL 写进生产探测链（P4/P2 红线）。
- 禁止在桌面设置行引入卡片容器或 Modal（ui-motion 契约，违者打回）。
