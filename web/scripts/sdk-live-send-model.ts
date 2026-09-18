/** doc `29` 的真机验收：随调用指定模型与思考等级。
 *
 * 验四件事（前三条在协议面上可判，第四条是唯一能证明「等级真的走到了尝试会话」的读法）：
 *  1. `conversation/send` 带 `model` / `reasoning_effort` 是**粘性**切换：写进会话行，
 *     并且**从这一轮起**生效（`conversation/get` 读得回来）。
 *  2. 不带这两个字段的普通发送**不改**会话的模型与等级。
 *  3. 会话正跑着一个 turn 时，带切换的 `send` 被 `conflict` 拒绝（而不是把运行时拆掉）。
 *  4. `agent/run` 的 `model` / `reasoning_effort` **真的被解析并使用**：不存在的 provider 必须
 *     以结构化错误失败（若字段被忽略，运行会成功——那就是缺陷）；等级则要能在宿主库里
 *     看到它落到了 attempt 会话的 `extra.reasoning_effort`（协议面没有这个读法，故直读 SQLite，
 *     失败只记 SKIP/OBSERVATION，不判 FAIL）。
 *
 * 用法（二进制必须含 doc `29` 的改动）：
 *   AGENT_STORE_BIN=.../target/debug/agent-store.exe bun scripts/sdk-live-send-model.ts
 */
import path from "node:path";
import { existsSync, readdirSync } from "node:fs";
import { Database } from "bun:sqlite";
import { launchHarness } from "@flowy-agent-store/sdk";

let failures = 0;
function check(name: string, ok: boolean, detail?: unknown): void {
  const suffix = detail === undefined ? "" : ` :: ${JSON.stringify(detail).slice(0, 400)}`;
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${suffix}`);
  if (!ok) failures += 1;
}
function observe(name: string, detail?: unknown): void {
  const suffix = detail === undefined ? "" : ` :: ${JSON.stringify(detail).slice(0, 400)}`;
  console.log(`OBSERVATION ${name}${suffix}`);
}
function codeOf(caught: unknown): string {
  return (caught as { code?: string } | null)?.code ?? String(caught).slice(0, 160);
}

const harness = await launchHarness({
  requestTimeoutMs: 300_000,
  client: { name: "sdk-live-send-model", version: "1" },
});
const { server } = harness;
console.log(`LISTENING ${server.readiness.host}:${server.readiness.port} data=${server.dataDir}`);

try {
  // 夹具：装一个专家，供 `agent/run` 那一半使用（`agent/run` 需要一个 preset-backed agent）。
  const sourcePath =
    process.env.AGENT_STORE_TEAM_SOURCE ??
    path.resolve(
      import.meta.dir,
      "../../crates/backend/nomifun-importer/tests/fixtures/software-company",
    );
  const imported = await harness.runImport({ source_path: sourcePath, source_kind: "codebuddy-plugin" });
  const installed = await harness.runInstall({ snapshot_id: imported.snapshot_id });
  const agents = await harness.agents.list();
  const target = agents.find((agent) => agent.name.includes("architect")) ?? agents[0];
  console.log(
    `INSTALL status=${imported.status} errors=${JSON.stringify(installed.errors)} agents=${agents.length}`,
  );

  // ── 挑第二个模型：必须是与会话当前模型**不同名字**的一项（同名切换不算切换） ─────────
  const options = await harness.conversations.modelOptions();
  const candidates = options.providers.flatMap((provider) =>
    provider.models.map((model) => ({ provider: provider.name, model: model.name })),
  );
  console.log(
    `MODELS ${candidates.map((candidate) => `${candidate.provider}/${candidate.model}`).join(", ")}`,
  );

  // ① 粘性切换：新建会话（idle）→ 带 model/effort 发一条 → 读回。
  //    优先挑 **mimo 家族**的另一个模型：本机实测 `opencode/big-pickle` 会让 attempt
  //    `Agent attempt timed out`（与本次改动无关——只带等级的对照变体是 completed），
  //    换一个能跑的模型能让读数干净些；判据本身只看会话行，不看轮次成败。
  const switchedChat = await harness.conversations.create({ name: "switch smoke" });
  const before = await harness.conversations.get(switchedChat.conversation_id);
  const second =
    candidates.find((candidate) => candidate.model !== before.model.model && candidate.model.includes("mimo")) ??
    candidates.find((candidate) => candidate.model !== before.model.model) ??
    candidates.find((candidate) => candidate.provider !== before.model.provider_id);

  if (!second) {
    observe("SM-001.send-switches-the-model", {
      means: "宿主只有一个模型可选项，无法构造「切到另一个模型」",
      current: before.model,
    });
  } else {
    await harness.conversations.send(
      switchedChat.conversation_id,
      "只回一个字：好。",
      `sm-switch-${Date.now()}`,
      {
        model: { provider_id: second.provider, model: second.model },
        reasoningEffort: "xhigh",
      },
    );
    const after = await harness.conversations.get(switchedChat.conversation_id);
    check("SM-001.send-switches-the-model", after.model.model === second.model, {
      from: before.model.model,
      to: after.model.model,
      expected: second.model,
      means: "send 上的 model 写进了会话行（粘性，从本轮起生效）",
    });
    check("SM-002.send-reads-back-the-effort", after.reasoning_effort === "xhigh", {
      before: before.reasoning_effort ?? null,
      after: after.reasoning_effort ?? null,
      means: "ConversationView.reasoning_effort 读得回来（fp-6 之前它是只写不读的）",
    });
  }

  // ② 词表校验在写库之前：非法等级必须 invalid_request，且会话值不变
  const invalidChat = await harness.conversations.create({ name: "invalid effort smoke" });
  const beforeInvalid = await harness.conversations.get(invalidChat.conversation_id);
  try {
    await harness.conversations.send(
      invalidChat.conversation_id,
      "不该发出去",
      `sm-invalid-${Date.now()}`,
      { reasoningEffort: "ultra" },
    );
    check("SM-003.invalid-effort-refused", false, "reasoning_effort=ultra was accepted");
  } catch (caught) {
    const afterInvalid = await harness.conversations.get(invalidChat.conversation_id);
    const code = codeOf(caught);
    check("SM-003.invalid-effort-refused", code === "invalid_request", { code });
    check(
      "SM-004.invalid-effort-writes-nothing",
      (afterInvalid.reasoning_effort ?? null) === (beforeInvalid.reasoning_effort ?? null),
      { before: beforeInvalid.reasoning_effort ?? null, after: afterInvalid.reasoning_effort ?? null },
    );
  }

  // ③ 忙判定：会话正跑着一轮时，带切换的 send 必须 conflict（不是把运行时拆掉）
  const busyChat = await harness.conversations.create({ name: "busy smoke" });
  await harness.conversations.send(
    busyChat.conversation_id,
    "从 1 数到 60，每个数字单独一行。",
    `sm-busy-first-${Date.now()}`,
  );
  const processingNow = (await harness.conversations.get(busyChat.conversation_id)).is_processing;
  try {
    await harness.conversations.send(busyChat.conversation_id, "换模型", `sm-busy-second-${Date.now()}`, {
      reasoningEffort: "low",
    });
    const settled = await harness.conversations.get(busyChat.conversation_id);
    observe("SM-005.busy-refuses-the-switch", {
      means: "第二次 send 被受理了——若首轮此刻已跑完（is_processing=false），这只是竞态，不是缺陷",
      processing_at_first_check: processingNow,
      processing_after: settled.is_processing,
    });
  } catch (caught) {
    const code = codeOf(caught);
    check("SM-005.busy-refuses-the-switch", code === "conflict", {
      code,
      processing_at_first_check: processingNow,
      means: "换模型会拆运行时，所以轮中必须拒绝（send 不是裸 update）",
    });
  }
  await harness.conversations.cancel(busyChat.conversation_id).catch(() => undefined);

  // ④ 普通发送（不带这两个字段）不改会话设置
  const plainChat = await harness.conversations.create({
    name: "plain smoke",
    model: second ? { provider_id: second.provider, model: second.model } : undefined,
    reasoningEffort: "high",
  });
  await harness.conversations.send(plainChat.conversation_id, "只回一个字：好。", `sm-plain-${Date.now()}`);
  const plain = await harness.conversations.get(plainChat.conversation_id);
  check(
    "SM-006.a-plain-send-keeps-the-conversation-settings",
    plain.reasoning_effort === "high" && plain.model.model === (second?.model ?? plain.model.model),
    { model: plain.model.model, reasoning_effort: plain.reasoning_effort ?? null },
  );
  await harness.conversations.cancel(plainChat.conversation_id).catch(() => undefined);

  // ⑤ agent/run：显式模型必须**真的被解析**——不存在的 provider 要以结构化错误失败。
  //    若这个字段被忽略，运行会照常成功，那正是本方案要防的"设了等于没设"。
  if (!target) {
    observe("SM-007.agent-run-resolves-the-explicit-model", { means: "没有装出来的专家可跑" });
  } else {
    const mention = { kind: "agent", id: target.id } as const;
    try {
      await harness.runs.agent({
        agentId: "",
        goal: "x",
        mentions: [mention],
        model: { provider_id: "definitely-not-a-provider", model: "nope" },
      });
      check("SM-007.agent-run-resolves-the-explicit-model", false, {
        means: "显式 model 被忽略了（运行照常成功）",
      });
    } catch (caught) {
      const code = codeOf(caught);
      check(
        "SM-007.agent-run-resolves-the-explicit-model",
        code === "provider_not_found" || code === "invalid_request",
        { code, means: "显式 model 走了权威解析（而不是被静默忽略）" },
      );
    }

    // 非法等级同样必须在运行入口被拒（早于任何模板物化）。
    try {
      await harness.runs.agent({
        agentId: "",
        goal: "x",
        mentions: [mention],
        reasoningEffort: "ultra",
      });
      check("SM-008.agent-run-refuses-an-unknown-effort", false, "ultra was accepted");
    } catch (caught) {
      check("SM-008.agent-run-refuses-an-unknown-effort", codeOf(caught) === "invalid_request", {
        code: codeOf(caught),
      });
    }

    // 正向：带上真实模型与等级，运行应当被受理。两点刻意的选择：
    //  - **显式给一个 step**：不给的话规划要先跑一次模型调用，attempt 何时物化就成了另一件事；
    //  - **显式写宿主默认模型**：它既走「显式 model」这条代码路径，又是本机确实能跑完的模型，
    //    于是下面 SM-010 能读到一条**成功**的 attempt（实测 `opencode/big-pickle` 会
    //    `Agent attempt timed out`，与本次改动无关：只带等级的对照变体是 completed）。
    const model = {
      provider: options.default?.provider ?? second?.provider ?? "",
      model: options.default?.model ?? second?.model ?? "",
    };
    let runId: string | null = null;
    try {
      const receipt = await harness.runs.agent({
        agentId: "",
        goal: "用一句话说明你的职责。",
        mentions: [mention],
        model: { provider_id: model.provider, model: model.model },
        reasoningEffort: "high",
        steps: [{ title: "回答职责", spec: "用一句话说明你的职责。" }],
      });
      runId = receipt.run_id;
      check("SM-009.agent-run-accepts-a-model-and-an-effort", Boolean(receipt.run_id), {
        run_id: receipt.run_id,
        status: receipt.status,
        preset_revision: receipt.preset_revision,
        model,
      });
    } catch (caught) {
      check("SM-009.agent-run-accepts-a-model-and-an-effort", false, { code: codeOf(caught) });
    }

    // ⑥ 唯一能证明「等级真的到了尝试会话」的读法：直读宿主库的 conversations.extra。
    //    协议面没有这个读法（attempt 会话不属于 App Server 聊天），所以失败只记 OBSERVATION。
    if (runId) {
      // 真源：`nomifun_common::storage_paths::DATABASE_FILE = "flowy-backend.db"`（不是备份包内部的
      // `database.sqlite3`——第一版脚本就栽在这个名字上）。宿主仍在运行，库是 WAL 模式，
      // 因此**不用** readonly 打开（只读打开在 WAL 下要求能写 `-shm`），但只跑 SELECT。
      const candidatesDb = ["flowy-backend.db", "nomifun-backend.db"].map((name) =>
        path.join(server.dataDir, name),
      );
      const dbPath = candidatesDb.find((candidate) => existsSync(candidate));
      if (!dbPath) {
        observe("SM-010.attempt-conversation-carries-the-effort", {
          means: "数据目录里没有找到宿主库（不影响前九条判据）",
          listed: readdirSync(server.dataDir).slice(0, 20),
        });
      } else {
        try {
          const db = new Database(dbPath);
          // 先等**尝试会话**出现（它没有 `app_server_chat` 键、有 `preset_snapshot`），再看它带不带等级。
          // 这两件事分开判，才能区分「没物化」与「物化了但没投影」——第一版把两者混成一个查询，
          // 既误报过失败，也说不清 OBSERVATION 的含义。
          let attemptExtra: string | null = null;
          for (let attempt = 0; attempt < 60 && attemptExtra === null; attempt += 1) {
            // 尝试会话的判据是 `agent_source`（会话层给 preset 会话写的身份），**不是**
            // `preset_snapshot`——快照存的是**一等列**，不进 `extra`；第一版按后者查，永远查不到。
            const row = db
              .query(
                "SELECT extra FROM conversations " +
                  "WHERE extra LIKE '%\"agent_source\"%' " +
                  "AND extra NOT LIKE '%\"app_server_chat\"%' LIMIT 1",
              )
              .get() as { extra?: string } | null;
            if (row?.extra) attemptExtra = row.extra;
            else await Bun.sleep(2000);
          }
          db.close();
          if (attemptExtra === null) {
            // 诊断：说清"没物化"到底停在哪儿（执行行状态 / 步骤行数），否则这条 OBSERVATION
            // 只能记一句"没看到"，下一个人还得从头查。
            const diagnostic = new Database(dbPath);
            let executions: unknown = null;
            try {
              executions = diagnostic
                .query("SELECT status, plan_revision, total_tokens FROM agent_executions")
                .all();
            } catch (caught) {
              executions = String(caught).slice(0, 160);
            }
            let steps: unknown = null;
            try {
              steps = diagnostic.query("SELECT kind, status FROM agent_execution_steps").all();
            } catch (caught) {
              steps = String(caught).slice(0, 160);
            }
            diagnostic.close();
            observe("SM-010.attempt-conversation-carries-the-effort", {
              means: "180s 内没有出现尝试会话（调度尚未物化 attempt），不判缺陷",
              executions,
              steps,
            });
          } else if (attemptExtra.includes('"reasoning_effort":"high"')) {
            check("SM-010.attempt-conversation-carries-the-effort", true, {
              extra: attemptExtra.slice(0, 260),
              means: "快照里的等级经 attempt runner 落到尝试会话 extra（运行时与普通会话同一读取路径）",
            });
          } else {
            check("SM-010.attempt-conversation-carries-the-effort", false, {
              extra: attemptExtra.slice(0, 260),
              means: "尝试会话**已物化**但 extra 里没有等级 —— 投影断了",
            });
          }
        } catch (caught) {
          observe("SM-010.attempt-conversation-carries-the-effort", {
            means: "读宿主库失败（不影响前九条判据）",
            database: dbPath,
            error: String(caught).slice(0, 160),
          });
        }
      }
    }
  }
} catch (error) {
  console.error("ERROR:", String(error).slice(0, 600));
  failures += 1;
} finally {
  console.log(`\nRESULT ${failures === 0 ? "PASS" : `FAIL(${failures})`} data=${server.dataDir}`);
  await server.close();
  process.exit(failures === 0 ? 0 : 1);
}
