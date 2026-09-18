/** WebUI 的 `@专家` 调用形状在真机上是否生效。
 *
 * WebUI 的 `@` 走的是 `appStore.ts` 里那条分支：只要 mention 里有 `kind === "agent"`，
 * 就调 `harness.runs.agent({ agentId: "", goal, mentions })`——**一次 Agent Run**，
 * 不是 `conversation/create({ agentId })`。这个脚本只验证那一件事，顺带把
 * 「未安装的专家」与「团 mention」两条边界用真机钉住。
 *
 * 用法（二进制必须含 doc `27` 的改动）：
 *   AGENT_STORE_BIN=.../target/debug/agent-store.exe bun scripts/sdk-live-mention-agent.ts
 */
import path from "node:path";
import { launchHarness } from "@flowy-agent-store/sdk";
import type { MentionRef } from "@flowy-agent-store/protocol";

let failures = 0;
function check(name: string, ok: boolean, detail?: unknown): void {
  const suffix = detail === undefined ? "" : ` :: ${JSON.stringify(detail).slice(0, 400)}`;
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${suffix}`);
  if (!ok) failures += 1;
}

const harness = await launchHarness({
  requestTimeoutMs: 300_000,
  client: { name: "sdk-live-mention-agent", version: "1" },
});
const { server } = harness;
console.log(`LISTENING ${server.readiness.host}:${server.readiness.port} data=${server.dataDir}`);

try {
  // 装夹具（同一个源，market 里没有专家团也没有专家条目以外的东西可依赖）。
  const sourcePath =
    process.env.AGENT_STORE_TEAM_SOURCE ??
    path.resolve(
      import.meta.dir,
      "../../crates/backend/nomifun-importer/tests/fixtures/software-company",
    );
  const imported = await harness.runImport({
    source_path: sourcePath,
    source_kind: "codebuddy-plugin",
  });
  const installed = await harness.runInstall({ snapshot_id: imported.snapshot_id });
  console.log(
    `INSTALL status=${imported.status} errors=${JSON.stringify(installed.errors)} outcomes=${installed.outcomes?.length ?? 0}`,
  );

  const agents = await harness.agents.list();
  console.log(`AGENTS ${agents.map((agent) => `${agent.id}|preset=${agent.preset_id ?? "none"}`).join(", ")}`);
  const target = agents.find((agent) => agent.name.includes("architect")) ?? agents[0];
  if (!target) {
    check("MA-001.agent-mention-starts-a-run", false, "no AgentDefinition is installed");
  } else {
    const mention = { kind: "agent", id: target.id } as const;

    // ⓿ 全新宿主、**还没解析过任何模型**时就 `@专家`。这曾经失败
    //    （`invalid_request` / `resolved_model is required`），因为 `agent/run` 的默认模型回退
    //    只读宿主 DB 的 provider 注册表，而 provider 是**按需注册**的。现在回退也读
    //    `~/.agent-store/config.toml` 的 `default_model`（与会话 / 团同源），这条就成了判据。
    try {
      const fresh = await harness.runs.agent({ agentId: "", goal: "x", mentions: [mention] });
      check("MA-001.fresh-host-agent-run", Boolean(fresh.run_id), {
        run_id: fresh.run_id,
        status: fresh.status,
        means: "全新宿主上第一次调用就能解析出模型（config 的 default_model）",
      });
    } catch (caught) {
      const code = (caught as { code?: string } | null)?.code ?? String(caught).slice(0, 160);
      check("MA-001.fresh-host-agent-run", false, { code });
    }

    // ① WebUI 的真实顺序：**先有会话**（创建会话会按需把 config.toml 的 provider 注册进库），
    //    再在这个会话里 `@专家`。
    const conversation = await harness.conversations.create({});
    check("MA-002.conversation-registers-the-provider", Boolean(conversation.model?.model), {
      conversation_id: conversation.conversation_id,
      model: conversation.model,
    });

    const receipt = await harness.runs.agent({
      agentId: "",
      goal: "用一句话说明你的职责。",
      mentions: [mention],
    });
    check("MA-003.agent-mention-starts-a-run", Boolean(receipt.run_id), {
      run_id: receipt.run_id,
      status: receipt.status,
      preset_revision: receipt.preset_revision,
      agent: target.id,
    });
  }

  // ② 未安装/不存在的专家：UI 会把这条错误显示出来（不是静默不发）。
  //    两种码要分清：**id 根本不存在 ⇒ `not_found`**；**id 存在但没有 preset（未 install）⇒
  //    `agent_not_installed`**。文档此前只写了后者，真机第一次跑就暴露了这个差别。
  try {
    await harness.runs.agent({
      agentId: "",
      goal: "x",
      mentions: [{ kind: "agent", id: "wb-not-installed" }],
    });
    check("MA-004.unknown-agent-refused", false, "an unknown agent id was accepted");
  } catch (caught) {
    const code = (caught as { code?: string } | null)?.code ?? String(caught).slice(0, 160);
    check("MA-004.unknown-agent-refused", code === "not_found" || code === "agent_not_installed", {
      code,
      means: "not_found = id 不存在；agent_not_installed = 存在但未 install/*",
    });
  }

  // ③ 「@专家团」在协议层不存在：`MentionKind` 只有 agent / skill / connector，
  //    所以即使硬塞一个 team mention，也只会是 wire 层拒绝。
  const teams = await harness.teams.list();
  console.log(`TEAMS ${teams.map((team) => `${team.id}|${team.name}`).join(", ")}`);
  try {
    await harness.runs.agent({
      agentId: "",
      goal: "x",
      mentions: [{ kind: "team", id: teams[0]?.id ?? "wb-team" } as unknown as MentionRef],
    });
    check("MA-005.team-mention-refused", false, "a team mention was accepted");
  } catch (caught) {
    const code = (caught as { code?: string } | null)?.code ?? String(caught).slice(0, 160);
    check("MA-005.team-mention-refused", true, { code, means: "wire 上没有 team 这个 kind" });
  }
} catch (error) {
  console.error("ERROR:", String(error).slice(0, 600));
  failures += 1;
} finally {
  console.log(`\nRESULT ${failures === 0 ? "PASS" : `FAIL(${failures})`} data=${server.dataDir}`);
  await server.close();
  process.exit(failures === 0 ? 0 : 1);
}
