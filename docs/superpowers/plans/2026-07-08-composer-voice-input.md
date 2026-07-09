# 首页与会话页语音输入 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在首页与会话页输入框实现「空输入 = 麦克风 / 有文字 = 麦克风 + 发送」交互，语音转写走 claw 云端 `qwen3-asr-flash`（category=7）。

**Architecture:** 新建共享 React 组件 `ComposerSubmitCluster` 统一按钮状态机；后端扩展 `/api/stt` 优先走 claw ASR（`ServerSession` + `FlowyApiClient`），回退用户配置的 OpenAI/Deepgram STT。`SpeechInputButton` 去掉设置开关门控，由 claw 可用性控制显隐。

**Tech Stack:** React 19 + TypeScript, Arco Design, Rust (nomifun-shell / nomifun-cloud), bun test, wiremock HTTP tests

**Spec:** `docs/superpowers/specs/2026-07-08-composer-voice-input-design.md`

---

## 文件总览

| 操作 | 路径 | 职责 |
|------|------|------|
| Create | `crates/backend/nomifun-cloud/src/flowy/asr.rs` | claw ASR 转写 API 封装 |
| Modify | `crates/backend/nomifun-cloud/src/flowy/media_types.rs` | `MODEL_CATEGORY_ASR = 7` |
| Modify | `crates/backend/nomifun-cloud/src/flowy/mod.rs` | 导出 ASR 方法 |
| Modify | `crates/backend/nomifun-cloud/src/lib.rs` | 重导出常量 |
| Modify | `crates/backend/nomifun-api-types/src/shell.rs` | `SpeechToTextProvider::Claw` |
| Create | `crates/backend/nomifun-shell/src/stt_claw.rs` | claw STT 编排（模型解析 + 调用） |
| Modify | `crates/backend/nomifun-shell/src/stt.rs` | 增加 Claw 分支 |
| Modify | `crates/backend/nomifun-shell/src/lib.rs` | 导出 `stt_claw` |
| Modify | `crates/backend/nomifun-shell/src/state.rs` | `ShellRouterState` 增加 `data_dir` |
| Modify | `crates/backend/nomifun-shell/src/routes.rs` | claw 优先路由逻辑 |
| Modify | `crates/backend/nomifun-app/src/router/state.rs` | `build_shell_state` 传入 `data_dir` |
| Create | `crates/backend/nomifun-shell/tests/stt_claw_integration.rs` | claw STT wiremock 测试 |
| Create | `ui/src/renderer/components/chat/ComposerSubmitCluster.tsx` | 共享按钮簇 |
| Create | `ui/src/renderer/components/chat/ComposerSubmitCluster.structure.test.ts` | 结构测试 |
| Modify | `ui/src/renderer/components/chat/SpeechInputButton.tsx` | 移除设置门控，支持 `hidden` |
| Modify | `ui/src/renderer/hooks/system/useSpeechInput.ts` | 增加 `useClawAsrAvailable` hook（可选） |
| Create | `ui/src/renderer/hooks/system/useClawAsrAvailable.ts` | 探测 claw ASR 是否可用 |
| Modify | `ui/src/renderer/pages/guid/components/GuidActionRow.tsx` | 接入 `ComposerSubmitCluster` |
| Modify | `ui/src/renderer/pages/guid/GuidPage.tsx` | 传入语音相关 props |
| Modify | `ui/src/renderer/components/chat/SendBox/index.tsx` | 接入 `ComposerSubmitCluster` |
| Modify | `ui/src/renderer/components/chat/SendBox/sendbox.css` | 按钮簇间距微调（如需） |

---

### Task 1: 增加 ASR 模型分类常量

**Files:**
- Modify: `crates/backend/nomifun-cloud/src/flowy/media_types.rs`
- Modify: `crates/backend/nomifun-cloud/src/lib.rs`

- [ ] **Step 1: 在 media_types.rs 增加常量**

```rust
/// `tb_model.category` for ASR models (`GET .../model/availableListClaw?category=7`).
pub const MODEL_CATEGORY_ASR: i32 = 7;
```

- [ ] **Step 2: 在 lib.rs 重导出**

```rust
pub use flowy::media_types::MODEL_CATEGORY_ASR;
```

- [ ] **Step 3: 编译验证**

```bash
cargo check -p nomifun-cloud
```

Expected: 编译通过，无 warning 新增

- [ ] **Step 4: Commit**

```bash
git add crates/backend/nomifun-cloud/src/flowy/media_types.rs crates/backend/nomifun-cloud/src/lib.rs
git commit -m "feat(cloud): add MODEL_CATEGORY_ASR constant for claw ASR models"
```

---

### Task 2: 实现 claw ASR 转写 API 封装

**Files:**
- Create: `crates/backend/nomifun-cloud/src/flowy/asr.rs`
- Modify: `crates/backend/nomifun-cloud/src/flowy/mod.rs`

- [ ] **Step 1: 写失败测试（asr.rs 内 #[cfg(test)]）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::flowy::types::ClawModelEntry;

    #[test]
    fn parse_asr_transcript_extracts_message_content() {
        let body = serde_json::json!({
            "choices": [{
                "message": { "content": "你好世界" }
            }]
        });
        assert_eq!(extract_asr_text(&body).as_deref(), Some("你好世界"));
    }

    #[test]
    fn parse_asr_transcript_falls_back_to_text_field() {
        let body = serde_json::json!({ "text": "hello" });
        assert_eq!(extract_asr_text(&body).as_deref(), Some("hello"));
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

```bash
cargo test -p nomifun-cloud asr::tests -- --nocapture
```

Expected: FAIL — module `asr` not found

- [ ] **Step 3: 实现 asr.rs**

核心逻辑：
1. `fetch_asr_models(session)` → `get_available_models_claw(session, Some(7))`
2. `transcribe_audio(session, audio_data, file_name, mime_type, language_hint)`:
   - 取 catalog 第一条 entry（`AIPC-qwen3-asr-flash`）
   - 将音频 base64 编码
   - `POST {llm_transport}/chat/completions`（OpenAI 多模态兼容格式）:

```rust
{
  "model": "AIPC-qwen3-asr-flash",
  "messages": [{
    "role": "user",
    "content": [
      { "type": "input_audio", "input_audio": { "data": "<base64>", "format": "webm" } }
    ]
  }]
}
```

   - `extract_asr_text(response)` 从 `choices[0].message.content` 或 `text` 字段取文本

> **实现注意：** 若 claw 实际路径不是 `/chat/completions`，先用 wiremock 对照 `endpoint` 字段调试；Qwen3-ASR-Flash 支持本地文件上传，claw `/v1` 代理应兼容 OpenAI 多模态消息格式。

- [ ] **Step 4: 在 mod.rs 注册模块并导出**

```rust
pub mod asr;
// 在 FlowyApiClient impl 或 re-export:
pub use asr::{fetch_asr_models, transcribe_audio, extract_asr_text};
```

- [ ] **Step 5: 运行测试确认通过**

```bash
cargo test -p nomifun-cloud asr::tests -- --nocapture
```

Expected: PASS（2 tests）

- [ ] **Step 6: Commit**

```bash
git add crates/backend/nomifun-cloud/src/flowy/asr.rs crates/backend/nomifun-cloud/src/flowy/mod.rs
git commit -m "feat(cloud): add claw ASR transcription API wrapper"
```

---

### Task 3: 扩展 SpeechToTextProvider 增加 Claw 变体

**Files:**
- Modify: `crates/backend/nomifun-api-types/src/shell.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn speech_to_text_provider_serializes_claw() {
    assert_eq!(
        serde_json::to_value(SpeechToTextProvider::Claw).unwrap(),
        "claw"
    );
}
```

- [ ] **Step 2: 运行测试确认失败**

```bash
cargo test -p nomifun-api-types speech_to_text_provider_serializes_claw -- --nocapture
```

Expected: FAIL — no variant `Claw`

- [ ] **Step 3: 增加枚举变体**

```rust
pub enum SpeechToTextProvider {
    Openai,
    Deepgram,
    Claw,
}
```

更新同文件内现有 serialize/deserialize 测试。

- [ ] **Step 4: 运行测试确认通过**

```bash
cargo test -p nomifun-api-types -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/backend/nomifun-api-types/src/shell.rs
git commit -m "feat(api-types): add Claw speech-to-text provider variant"
```

---

### Task 4: 实现 stt_claw 模块与 SttService 分支

**Files:**
- Create: `crates/backend/nomifun-shell/src/stt_claw.rs`
- Modify: `crates/backend/nomifun-shell/src/stt.rs`
- Modify: `crates/backend/nomifun-shell/src/lib.rs`
- Modify: `crates/backend/nomifun-shell/Cargo.toml`（如需添加 `nomifun-cloud` 依赖）

- [ ] **Step 1: 确认 nomifun-shell 依赖 nomifun-cloud**

在 `crates/backend/nomifun-shell/Cargo.toml` 添加：

```toml
nomifun-cloud = { path = "../nomifun-cloud" }
nomi-config = { path = "../../agent/nomi-config" }
```

- [ ] **Step 2: 写失败测试**

`crates/backend/nomifun-shell/src/stt_claw.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claw_not_configured_when_no_token() {
        // 用临时目录 + 空 session 验证返回 ClawNotConfigured
    }
}
```

- [ ] **Step 3: 实现 stt_claw.rs**

```rust
pub async fn transcribe_via_claw(
    data_dir: &Path,
    audio_data: Vec<u8>,
    file_name: &str,
    mime_type: &str,
    language_hint: Option<&str>,
) -> Result<SpeechToTextResult, SttError> {
    let config = load_user_config_file(&config_yaml_path(Some(data_dir)))?;
    let api = FlowyApiClient::new(&config.server)?;
    let session = ServerSession::from_config(&config.server, data_dir);
    let token = session.access_token().await?;
    if token.filter(|t| !t.trim().is_empty()).is_none() {
        return Err(SttError::ClawNotConfigured);
    }
    let text = nomifun_cloud::transcribe_audio(
        &api, &session, audio_data, file_name, mime_type, language_hint
    ).await.map_err(|e| SttError::RequestFailed(e.to_string()))?;
    Ok(SpeechToTextResult {
        text,
        model: "qwen3-asr-flash".into(),
        provider: SpeechToTextProvider::Claw,
        language: language_hint.map(str::to_owned),
    })
}
```

- [ ] **Step 4: 在 error.rs 增加 `ClawNotConfigured` 变体**

```rust
pub enum SttError {
    // ...existing...
    ClawNotConfigured,
}
```

映射 HTTP 503 + code `STT_CLAW_NOT_CONFIGURED`。

- [ ] **Step 5: 在 stt.rs 增加 Claw 分支（供显式调用）**

- [ ] **Step 6: 编译验证**

```bash
cargo check -p nomifun-shell
```

- [ ] **Step 7: Commit**

```bash
git add crates/backend/nomifun-shell/
git commit -m "feat(shell): add claw STT transcription module"
```

---

### Task 5: 改造 /api/stt 路由 — claw 优先

**Files:**
- Modify: `crates/backend/nomifun-shell/src/state.rs`
- Modify: `crates/backend/nomifun-shell/src/routes.rs`
- Modify: `crates/backend/nomifun-app/src/router/state.rs`
- Create: `crates/backend/nomifun-shell/tests/stt_claw_integration.rs`

- [ ] **Step 1: ShellRouterState 增加 data_dir**

```rust
pub struct ShellRouterState {
    pub shell_service: Arc<ShellService>,
    pub stt_service: Arc<SttService>,
    pub client_pref_service: ClientPrefService,
    pub data_dir: PathBuf,
}
```

- [ ] **Step 2: build_shell_state 传入 data_dir**

`nomifun-app/src/router/state.rs`:

```rust
ShellRouterState {
    // ...existing...
    data_dir: services.data_dir.clone(),
}
```

- [ ] **Step 3: 改造 speech_to_text handler**

```rust
// 1. 先尝试 claw
match stt_claw::transcribe_via_claw(&state.data_dir, ...).await {
    Ok(result) => return Ok(success_json(result)),
    Err(SttError::ClawNotConfigured) => { /* fall through */ }
    Err(e) => return Err(stt_error_response(&e)),
}
// 2. 回退用户 speechToText 偏好（现有逻辑）
```

- [ ] **Step 4: 写 wiremock 集成测试**

`stt_claw_integration.rs`：mock claw model list + transcription endpoint，POST `/api/stt` 返回 transcript。

- [ ] **Step 5: 运行测试**

```bash
cargo test -p nomifun-shell stt_claw -- --nocapture
cargo test -p nomifun-app shell_e2e -- --nocapture
```

- [ ] **Step 6: Commit**

```bash
git add crates/backend/nomifun-shell/ crates/backend/nomifun-app/src/router/state.rs
git commit -m "feat(shell): prefer claw ASR in /api/stt with user-config fallback"
```

---

### Task 6: 前端 — claw ASR 可用性探测 hook

**Files:**
- Create: `ui/src/renderer/hooks/system/useClawAsrAvailable.ts`
- Modify: `ui/src/common/adapter/ipcBridge.ts`（如需新增 IPC）

- [ ] **Step 1: 确认或新增后端探测端点**

优先复用已有 cloud model list IPC。搜索 `list_claw_models` / `cloud.` bridge：

```bash
rg "list_claw_models|listClawModels|availableListClaw" ui/src/common/adapter/ipcBridge.ts crates/
```

若无前端 bridge，在 `ipcBridge.cloud` 增加：

```typescript
listAsrModels: httpGet<ClawModelEntry[]>('/api/cloud/models/claw?category=7'),
```

（或复用已有通用 `listClawModels(category)` 方法。）

- [ ] **Step 2: 实现 useClawAsrAvailable**

```typescript
export function useClawAsrAvailable(): { ready: boolean; available: boolean } {
  const { data, isLoading } = useSWR('claw-asr-models', () =>
    ipcBridge.cloud.listClawModels.invoke({ category: 7 })
  );
  return {
    ready: !isLoading,
    available: (data?.length ?? 0) > 0,
  };
}
```

- [ ] **Step 3: Commit**

```bash
git add ui/src/renderer/hooks/system/useClawAsrAvailable.ts ui/src/common/adapter/ipcBridge.ts
git commit -m "feat(ui): add useClawAsrAvailable hook for claw ASR model detection"
```

---

### Task 7: 改造 SpeechInputButton — 移除设置门控

**Files:**
- Modify: `ui/src/renderer/components/chat/SpeechInputButton.tsx`

- [ ] **Step 1: 删除 configService 门控逻辑**

移除：
- `isSpeechToTextEnabled` state
- `SPEECH_TO_TEXT_CONFIG_CHANGED_EVENT` listener
- `if (!isConfigLoaded || !isSpeechToTextEnabled) return null`

- [ ] **Step 2: 增加 `hidden?: boolean` prop**

```typescript
type SpeechInputButtonProps = {
  disabled?: boolean;
  hidden?: boolean;
  locale?: string;
  onTranscript: (transcript: string) => void;
};

if (hidden) return null;
```

- [ ] **Step 3: 验证 TypeScript**

```bash
cd ui && bun run typecheck
```

- [ ] **Step 4: Commit**

```bash
git add ui/src/renderer/components/chat/SpeechInputButton.tsx
git commit -m "feat(ui): remove speech-to-text settings gate from SpeechInputButton"
```

---

### Task 8: 新建 ComposerSubmitCluster 共享组件

**Files:**
- Create: `ui/src/renderer/components/chat/ComposerSubmitCluster.tsx`
- Create: `ui/src/renderer/components/chat/ComposerSubmitCluster.structure.test.ts`

- [ ] **Step 1: 写结构测试**

```typescript
describe('ComposerSubmitCluster', () => {
  test('empty draft renders speech button without disabled send', () => {
    const source = readSource(new URL('./ComposerSubmitCluster.tsx', import.meta.url));
    expect(source.includes('hasDraft')).toBe(true);
    expect(source.includes('SpeechInputButton')).toBe(true);
    expect(source.includes('data-testid="composer-send-btn"')).toBe(true);
    expect(source.includes('!hasDraft')).toBe(true); // 空输入不渲染 disabled send
  });

  test('autoWorkMode renders robot button alongside speech', () => {
    const source = readSource(new URL('./ComposerSubmitCluster.tsx', import.meta.url));
    expect(source.includes('autoWorkMode')).toBe(true);
    expect(source.includes('Robot')).toBe(true);
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

```bash
cd ui && bun test src/renderer/components/chat/ComposerSubmitCluster.structure.test.ts
```

Expected: FAIL — file not found

- [ ] **Step 3: 实现 ComposerSubmitCluster.tsx**

```typescript
export type ComposerSubmitClusterProps = {
  hasDraft: boolean;
  loading?: boolean;
  disabled?: boolean;
  isUploading?: boolean;
  autoWorkMode?: boolean;
  speechLocale?: string;
  onSend: () => void;
  onSpeechTranscript: (text: string) => void;
  // SendBox-specific
  showStop?: boolean;
  onStop?: () => void;
  showSteer?: boolean;
  onSteer?: () => void;
  steerAvailable?: boolean;
  onSteer?: () => void;
  speechHidden?: boolean;
  sendTestId?: string;
};

const ComposerSubmitCluster: React.FC<ComposerSubmitClusterProps> = (props) => {
  const { ready, available } = useClawAsrAvailable();
  const speechHidden = props.speechHidden || !ready || !available;

  const speechDisabled = props.disabled || props.loading || props.isUploading
    || (props.showStop && !props.hasDraft);

  return (
    <div className="composer-submit-cluster flex items-center gap-2">
      <SpeechInputButton
        hidden={speechHidden}
        disabled={speechDisabled}
        locale={props.speechLocale}
        onTranscript={props.onSpeechTranscript}
      />
      {props.showStop && (
        <Button /* stopButton styles */ onClick={props.onStop} data-testid="composer-stop-btn" />
      )}
      {props.showSteer && props.steerAvailable && props.hasDraft && (
        <Button /* steerButton */ onClick={props.onSteer} data-testid="composer-steer-btn" />
      )}
      {props.autoWorkMode && (
        <Tooltip content={t('requirements.autowork.startSession')}>
          <Button
            shape="circle" type="primary"
            loading={props.loading}
            disabled={autoWorkStartDisabled(props.loading, ...)}
            className="send-button-custom"
            icon={<Robot ... />}
            onClick={props.onSend}
            data-testid="composer-autowork-btn"
          />
        </Tooltip>
      )}
      {props.hasDraft && !props.autoWorkMode && (
        <Button
          shape="circle" type="primary"
          disabled={props.disabled || props.isUploading}
          className="send-button-custom"
          icon={<ArrowUp ... />}
          onClick={props.onSend}
          data-testid={props.sendTestId ?? 'composer-send-btn'}
        />
      )}
      {props.hasDraft && props.autoWorkMode && (
        <Button /* send alongside robot */ ... data-testid="composer-send-btn" />
      )}
    </div>
  );
};
```

**状态规则（实现时严格遵循）：**

| 条件 | 渲染 |
|------|------|
| `!speechHidden` | 麦克风 |
| `showStop && !hasDraft` | 停止 |
| `showStop && hasDraft` | 停止 + 发送（+ steer 若启用） |
| `autoWorkMode && !hasDraft` | 机器人 |
| `autoWorkMode && hasDraft` | 机器人 + 发送 |
| `hasDraft && !autoWorkMode` | 发送 |
| `!hasDraft && !autoWorkMode && !showStop` | 仅麦克风（不渲染 disabled 发送） |

- [ ] **Step 4: 运行结构测试**

```bash
cd ui && bun test src/renderer/components/chat/ComposerSubmitCluster.structure.test.ts
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add ui/src/renderer/components/chat/ComposerSubmitCluster.tsx ui/src/renderer/components/chat/ComposerSubmitCluster.structure.test.ts
git commit -m "feat(ui): add ComposerSubmitCluster shared submit/speech button cluster"
```

---

### Task 9: GuidActionRow 接入 ComposerSubmitCluster

**Files:**
- Modify: `ui/src/renderer/pages/guid/components/GuidActionRow.tsx`
- Modify: `ui/src/renderer/pages/guid/GuidPage.tsx`

- [ ] **Step 1: GuidActionRow 替换按钮区**

删除：
- `speechInputNode` prop
- 内联 `Button` 发送/机器人按钮（约 L300-321）

新增 props:

```typescript
hasDraft: boolean;
speechLocale?: string;
onSpeechTranscript: (text: string) => void;
```

渲染：

```tsx
<ComposerSubmitCluster
  hasDraft={hasDraft}
  loading={loading}
  disabled={isButtonDisabled && !autoWorkMode}
  autoWorkMode={autoWorkMode}
  speechLocale={speechLocale}
  onSend={onSend}
  onSpeechTranscript={onSpeechTranscript}
  sendTestId="guid-send-btn"
/>
```

- [ ] **Step 2: GuidPage 传入新 props**

```typescript
const hasDraft = guidInput.input.trim().length > 0;

<GuidActionRow
  hasDraft={hasDraft}
  speechLocale={i18n.language}
  onSpeechTranscript={(transcript) => {
    guidInput.setInput(appendSpeechTranscript(guidInput.input, transcript));
  }}
  // 移除 isButtonDisabled 用于 disabled send 的语义 —— 仅保留 loading/autoWork 控制
  isButtonDisabled={send.isButtonDisabled}
  ...
/>
```

- [ ] **Step 3: 类型检查**

```bash
cd ui && bun run typecheck
```

- [ ] **Step 4: Commit**

```bash
git add ui/src/renderer/pages/guid/components/GuidActionRow.tsx ui/src/renderer/pages/guid/GuidPage.tsx
git commit -m "feat(guid): wire ComposerSubmitCluster into homepage action row"
```

---

### Task 10: SendBox 接入 ComposerSubmitCluster

**Files:**
- Modify: `ui/src/renderer/components/chat/SendBox/index.tsx`

- [ ] **Step 1: 删除旧按钮渲染逻辑**

删除：
- `sendButton` / `stopButton` / `steerButton` 局部 JSX（保留 handler 函数）
- `renderActionButtons()`
- `renderedSpeechButton`

- [ ] **Step 2: 替换为 ComposerSubmitCluster**

在两处布局（单行 L1847-1852、多行 L1868-1873）统一渲染：

```tsx
<ComposerSubmitCluster
  hasDraft={hasDraftToSend}
  loading={isLoading || loading}
  disabled={disabled}
  isUploading={isUploading}
  speechLocale={speechLocale}
  onSend={sendMessageHandler}
  onSpeechTranscript={handleSpeechTranscript}
  showStop={isLoading || loading}
  onStop={stopHandler}
  showSteer={Boolean(onSteer)}
  steerAvailable={steerAvailable}
  onSteer={steerMessageHandler}
  speechHidden={isMobileCompact}
  sendTestId="sendbox-send-btn"
/>
```

**loading 分支映射（保持现有行为）：**
- `allowSendWhileLoading && loading && !hasDraft` → `showStop=true`，无发送
- `allowSendWhileLoading && loading && hasDraft` → `showStop` 不显示（或按现有 compactActions 逻辑），显示发送 + steer

> 仔细对照原 `renderActionButtons` 中 `compactActions` 分支，确保行为不退化。

- [ ] **Step 3: 类型检查 + 结构验证**

```bash
cd ui && bun run typecheck
cd ui && bun test src/renderer/components/chat/ComposerSubmitCluster.structure.test.ts
```

- [ ] **Step 4: Commit**

```bash
git add ui/src/renderer/components/chat/SendBox/index.tsx
git commit -m "feat(sendbox): replace send/speech buttons with ComposerSubmitCluster"
```

---

### Task 11: CSS 微调与暗色主题验证

**Files:**
- Modify: `ui/src/renderer/components/chat/SendBox/sendbox.css`

- [ ] **Step 1: 增加按钮簇样式（如需）**

```css
.composer-submit-cluster {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
```

- [ ] **Step 2: 确认暗色主题下麦克风与发送按钮对比度**

手动检查 `[data-theme='dark']` 下 `speech-input-button` 与 `send-button-custom` 可见性。

- [ ] **Step 3: Commit**

```bash
git add ui/src/renderer/components/chat/SendBox/sendbox.css
git commit -m "style(sendbox): add composer submit cluster layout spacing"
```

---

### Task 12: 端到端验证

**Files:** 无新增

- [ ] **Step 1: 后端测试全通过**

```bash
cargo test -p nomifun-shell
cargo test -p nomifun-cloud
cargo test -p nomifun-api-types
```

- [ ] **Step 2: 前端测试全通过**

```bash
cd ui && bun test src/renderer/components/chat/
cd ui && bun run typecheck
```

- [ ] **Step 3: 手动验证清单**

| # | 场景 | 预期 |
|---|------|------|
| 1 | 首页空输入 | 仅灰色麦克风，无 disabled 发送 |
| 2 | 首页输入文字 | 麦克风 + 黑色发送按钮 |
| 3 | 点击麦克风录音 | 波形动画 → 停止 → 转写 → 文字填入 |
| 4 | 首页 AutoWork 空输入 | 麦克风 + 机器人按钮 |
| 5 | 首页 AutoWork + 文字 | 麦克风 + 机器人 + 发送 |
| 6 | 会话页空输入 | 同首页 |
| 7 | 会话页生成中 + 空输入 | 麦克风 disabled + 停止 |
| 8 | 会话页生成中 + 有文字 | 麦克风 disabled + 发送（可排队） |
| 9 | claw ASR 不可用 | 无麦克风，回退旧发送逻辑 |

---

## Spec 覆盖自检

| Spec 要求 | 对应 Task |
|-----------|----------|
| MODEL_CATEGORY_ASR = 7 | Task 1 |
| claw ASR 转写 API | Task 2 |
| /api/stt claw 优先 | Task 4, 5 |
| ComposerSubmitCluster 共享组件 | Task 8 |
| 首页接入 | Task 9 |
| 会话页接入 | Task 10 |
| 移除 STT 设置门控 | Task 7 |
| claw 可用性探测 | Task 6 |
| AutoWork 三按钮并存 | Task 8, 9 |
| 错误处理 toast | Task 7（复用现有 SpeechInputButton 错误处理） |
| 测试 | Task 2, 5, 8, 12 |

## 执行选项

**Plan complete and saved to `docs/superpowers/plans/2026-07-08-composer-voice-input.md`. Two execution options:**

**1. Subagent-Driven（推荐）** — 每个 Task 派发独立 subagent，任务间做 review，迭代更快

**2. Inline Execution** — 在本会话按 Task 顺序直接实现，每 2-3 个 Task 设检查点

**Which approach?**
