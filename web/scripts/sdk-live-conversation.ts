/** WP-4 05d live 验证：多轮 ConversationHandle（REQ-PAR-05d）真机闭环。
 *
 * 协议面全走 SDK 公共面（launchClient + client.conversations + ConversationHandle）。
 * 模型 provider 唯一来源是 `~/.agent-store/config.toml`（`[providers.mimo]`），
 * 不再经 `/api/providers` 运行时注册。
 *
 * 用法：AGENT_STORE_BIN=.../agent-store.exe bun scripts/sdk-live-conversation.ts
 */
import { launchClient } from "@flowy-agent-store/sdk";
import { ConversationHandle } from "@flowy-agent-store/client";

let failures = 0;
function check(name: string, ok: boolean, detail?: unknown): void {
  const suffix = detail === undefined ? "" : ` :: ${JSON.stringify(detail).slice(0, 300)}`;
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${suffix}`);
  if (!ok) failures += 1;
}

const launched = await launchClient({
  requestTimeoutMs: 120_000,
  client: { name: "sdk-live-conversation", version: "1" },
});
const { server, client } = launched;
console.log(`LISTENING ${server.readiness.host}:${server.readiness.port} data=${server.dataDir}`);

try {
  // REQ-PAR-05d: open → send(await terminal) → transcript → cancel-safe close.
  // provider 显式引用 `~/.agent-store/config.toml` 的 `[providers.mimo]`（唯一来源）。
  const handle = await ConversationHandle.open(client.conversations, {
    name: `sdk-live-cv-${Date.now()}`,
    model: { provider_id: "mimo", model: "mimo-v2.5" },
  });
  check("CV-001.open", Boolean(handle.conversationId), handle.conversationId);

  const turn = await handle.send("用一句话说明你负责什么。");
  check("CV-002.send-completed", turn.completed === true, turn.completed);
  check(
    "CV-003.assistant-text",
    Boolean(turn.assistant_text) && turn.assistant_text !== null,
    (turn.assistant_text ?? "").slice(0, 80),
  );
  check("CV-004.turn-has-events", turn.events.length > 0, [[...new Set(turn.events.map((event) => event.event_type))].slice(0, 8), turn.events.length]);
  check("CV-005.not-error", turn.isError === false, { isError: turn.isError, code: turn.result_error_code });
  check("CV-006.usage-or-absent", turn.usage === undefined || turn.usage === null || typeof turn.usage.used_tokens === "number", turn.usage);

  const page = await handle.messages();
  check("CV-007.transcript", page.items.length > 0, page.items.map((message) => message.role));
  check("CV-008.transcript-contains-user", page.items.some((message) => message.role === "user"), page.items.length);

  await handle.close();
  check("CV-009.close", true);
} catch (error) {
  console.error("FAIL:", String(error).slice(0, 500));
  failures += 1;
} finally {
  console.log(`\nRESULT ${failures === 0 ? "PASS" : `FAIL(${failures})`} data=${server.dataDir}`);
  await server.close();
  process.exit(failures === 0 ? 0 : 1);
}
