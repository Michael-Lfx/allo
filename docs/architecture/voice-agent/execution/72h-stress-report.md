# AI Mate 72 小时挂机压测与长期可靠性测试规程 (72h Soak Test Protocol & Target Specifications)

> **对齐**：Phase 4 / Gate 4 出口阻断门禁
> **测试状态**：待真机长周期压测（Pending Hardware Soak Test Execution，尚未完成物理实机签核）
> **目标测试环境**：
> - 硬件：无屏伴侣参考硬件（RK3588 / 4GB LPDDR4 / USB 远场阵列麦克风 / 双声道扬声器）
> - 操作系统：Linux 6.1 (Debian 12 Bookworm, systemd 252)
> - 声学校验集：2 小时电视交谈噪音混叠回放 + 远场唤醒样本采样

---

## 1. 核心验收指标与规程基线

| 指标维度 | Gate 4 验收标准 | 验证方法与断言锚点 | 状态 (Status) |
| :--- | :--- | :--- | :--- |
| **误唤醒率** | $\le 1$ 次 / 24 小时 | 真实连续混叠白噪与电视音频流下 KWS 触发统计 | 待实测 (Pending) |
| **内存泄漏趋势** | 72h 曲线斜率为 0 (无常驻泄漏) | 连续采样式 RSS 记录，分析 $\Delta\text{RSS}$ 趋势与堆分配 | 待实测 (Pending) |
| **CPU 待机占用** | $< 1.5\%$ | `top`/`pidstat` 72 小时待机采样均值 | 待实测 (Pending) |
| **SPSC 音频管道** | 0 阻塞、0 溢出丢包 (`overflow_count = 0`) | `AudioCapturePipeline::metrics()` 统计计数 | 待实测 (Pending) |
| **USB 麦克风热插拔恢复**| 拔出后插回 2 秒内恢复监听 | 物理断开注入后至 `StreamStarted` 事件间隔测量 | 待实测 (Pending) |
| **服务常驻守护** | systemd 看门狗 15s 心跳保活 | `NOTIFY_SOCKET` `WATCHDOG=1` 周期喂狗与异常崩溃监测 | 待实测 (Pending) |

---

## 2. 内存与 CPU 采样规程 (Sampling Protocol)

- **0h ~ 24h**：初始化加载 ONNX 模型只读 `mmap`，记录冷启动基线 RSS，建立稳态基线；
- **24h ~ 48h**：持续输入混合背景白噪音，SPSC 环形缓冲持续读写，记录 RSS 抖动区间；
- **48h ~ 72h**：模拟周期性 HTTP 状态轮询与长周期 SSE 事件推送，评估是否存在长期内存逃逸。

---

## 3. 热插拔自愈测试规程 (Hotplug Verification Protocol)

- 在第 12h、24h、48h、60h 分别对 USB 阵列麦克风执行物理与电气断开注入；
- 看门狗与 CPAL 回调须在 100ms 内捕获断开事件并记录 `DeviceDisconnected`；
- 设备重新接入后，看门狗须在 2.0s 内重新绑定默认输入流并重置 SPSC 环形缓冲区。

---

## 4. 签核说明 (Sign-off Criteria)

> [!IMPORTANT]
> Gate 4 长期挂机门禁必须基于真实目标硬件的连续 72 小时实测数据进行签核。在实机压测前，本项保持未签核状态（Unsigned），严禁在缺乏硬件实测数据的前提下标记通过。
