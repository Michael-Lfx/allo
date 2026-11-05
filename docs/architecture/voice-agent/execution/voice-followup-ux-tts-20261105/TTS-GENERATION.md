# 本地 TTS 六段 WAV 生成证据

- 日期：2026-11-05（UTC+8）
- 模型目录：`C:\Users\38788\AppData\Local\Flowy\models\tts`
- 生成工具：`crates/backend/nomifun-voice-terminal/examples/generate_vocal_assets.rs`
- 命令：
  ```powershell
  cargo run -p nomifun-voice-terminal --example generate_vocal_assets --features local-tts -- `
    --model-dir "C:\Users\38788\AppData\Local\Flowy\models\tts" `
    --out-dir "build.noindex\generated-vocal"
  ```
- 速度：`SherpaOfflineTtsConfig.speed = 1.1`
- 输出位置：`build.noindex/generated-vocal/`（独立目录，**未替换/未提交**仓库资产）
- 输出格式：16 kHz / mono / 16-bit PCM WAV，30ms 前导静音 + 30ms 尾部静音

## 模型指纹

| 文件 | SHA-256 |
| --- | --- |
| `vocos-22khz-univ.onnx` | `0574A135AA1DB2DE6E181050DB2EC528496CACD4A4701FC5D7FAF9F9804C0081` |
| `matcha-icefall-zh-baker/model-steps-3.onnx` | `78A9FD17E9A6C20198DCA4E69D18F941303F8AB39EC61DDE04CFC7595262DDFD` |
| `matcha-icefall-en_US-ljspeech/model-steps-3.onnx` | `4D7771C0EC063CA74F2FA92BA0ECFE14F73FA3F61C236D9469B92E102D8E9574` |

## 六段结果

| 文件 | 文案 | 源采样率 | 时长 ms | peak | RMS | 前/后静音 ms | SHA-256 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `wake_zh.wav` | 我在 | 22050 | 382 | 0.369 | 0.0694 | 30 / 39 | `6054c46d4beeb15acd3d76d84322c433c821158918d6ebcd4b81489090062fd1` |
| `wake_en.wav` | I'm here | 22050 | 733 | 0.411 | 0.0625 | 31 / 64 | `e49c63b06d7ee1f65d633c15915a92b7d691ca3f4a0bd39c289d9e75f3f6bbba` |
| `barge_zh.wav` | 你说 | 22050 | 453 | 0.247 | 0.0791 | 30 / 35 | `eb2775a57c7641988ac3893031f9f8ee9cef1e4512e3312eb57457a804bd2da6` |
| `barge_en.wav` | Go ahead | 22050 | 662 | 0.282 | 0.0638 | 64 / 56 | `617690bda41f776e58176e83fa89e643fe856ac5ed91b4ece4070b186d1ae51e` |
| `dismiss_zh.wav` | 好的，再见 | 22050 | 870 | 0.421 | 0.0650 | 30 / 37 | `48f0b3d3063e2fb134dbed6f9b567fd51c3288f7d8eeaddf9b319c2b82a36a52` |
| `dismiss_en.wav` | Goodbye | 22050 | 554 | 0.498 | 0.0662 | 31 / 55 | `9a2f3ba8551dc274a2416b2f1d1496c6e933cb5b5dec74729330d40a00741331` |

## 1.0 / 1.1 / 1.3 速度对比（同一文案、同一模型）

| 文件 | 1.0x ms | 1.1x ms | 1.3x ms |
| --- | ---: | ---: | ---: |
| `wake_zh.wav` | 426 | 382 | 330 |
| `wake_en.wav` | 802 | 733 | 627 |
| `barge_zh.wav` | 508 | 453 | 398 |
| `barge_en.wav` | 727 | 662 | 570 |
| `dismiss_zh.wav` | 993 | 870 | 883 |
| `dismiss_en.wav` | 617 | 554 | 477 |

除 `dismiss_zh` 的 1.3x 比 1.1x 长约 13ms（模型推理/裁剪边界差异）外，其余均随速度提高而缩短；每段均重新合成，没有复用旧缓存。复跑命令追加 `--speed 1.0` / `--speed 1.3` 与对应 `--out-dir`。

## 现有仓库资产

| 文件 | SHA-256（保留未替换） |
| --- | --- |
| `wake_zh.wav` | `3cddf5fdbbea2198fac3610737f0bb3fd021e7ac26b6dcb670c327fa281827ee` |
| `wake_en.wav` | `415da4d56b0fa5a2945b248b05dd71148761a29d6d8a4a090cf34136b13301b2` |
| `barge_zh.wav` | `2c0391a970f1e6f1e240ef4d77f4cf89ee6d73b12d455b3b8bb8900eb9b1d713` |
| `barge_en.wav` | `1c779e2edef92adde5132a7b86eca345803131adfe8359bc17802e086eee77e7` |
| `dismiss_zh.wav` | `c378a334d48c5f6146eedb5fd4a9175c94ed1a23d0d0debaca0128ae3958335e` |
| `dismiss_en.wav` | `254fbf8ecc61c65c4132f081e7b4b62463f7e7f514bb536c1b85876b950f80bb` |

## 未验 / 阻塞

- 六段自动化指标：非静音、无削波、16kHz/mono/16-bit、首尾静音通过；**可懂度/真人听感未验**。
- 生成过程中 sherpa 打印 `Unknown token: shei2` 警告；需真机试听确认中文发音。
- 中文模型 README 明确 non-commercial，仓库架构文档记录无显式发行许可；**未替换资产**，等待产品/法务确认。
