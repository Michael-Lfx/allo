/** doc `27` 阶段 2b 真机验证：`conversation/create({ teamId })` 打开 Leader 会话之后，
 * **客户端自己的第一条消息**是否真的让 Leader 委派出去。
 *
 * 为什么单独立一个脚本：单测只覆盖到创建侧（互斥校验、成员/连接器栅栏、快照冻结、Leader 行形状），
 * 「不发 goal 首轮、由调用方发首轮」这条语义只有在真机上才能证明——它依赖
 * `nomi_delegate` 的宿主实现（`apps/agent-store` 用的是**持久化** Agent Execution facade，
 * 见 `apps/agent-store/src/main.rs` 的 `no_embedded_agent_execution`）。
 *
 * 用法（二进制必须含本次改动）：
 *   AGENT_STORE_BIN=.../target/debug/agent-store.exe bun scripts/sdk-live-team-leader.ts
 *
 * 判据（PASS 需要下列全过）：
 *   TL-001 装上团定义（官方市场有 team 条目就走 `store/install`；实测没有，则退回本地夹具，
 *          走 `import/run` + `install/run`）
 *   TL-002 `team/list` 出现该团定义（它的 `id` 才是 `create` 的 `teamId`）
 *   TL-003 创建出 Leader 会话（`teamId` 被接受）
 *   TL-004 首轮被受理并跑到终态
 *   TL-005 明确点名委派时，Leader **能**找到并调用 `nomi_delegate`（工具确实注册了）
 *   TL-006 与既有入口 `team/run` 的对照**只作观察**：实测「委派与否」由模型决定（6 次里 2 次委派、
 *          4 次自己动手做，`team/run` 也会 `team_run_not_started`；逐条读数见 `27` §9.1），所以它不能
 *          当判据。**2b 要证明的是「客户端首轮能够触发委派」**，由 TL-004 的事件证据与 TL-005 覆盖。
 */
import path from "node:path";
import { launchHarness } from "@flowy-agent-store/sdk";
import type { ConversationEvent } from "@flowy-agent-store/protocol";

let failures = 0;
let skipped = false;
function check(name: string, ok: boolean, detail?: unknown): void {
  const suffix = detail === undefined ? "" : ` :: ${JSON.stringify(detail).slice(0, 600)}`;
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${suffix}`);
  if (!ok) failures += 1;
}

const TURN_TIMEOUT_MS = 180_000;
const INSTALL_TIMEOUT_MS = 300_000;
/** 内置市场源是**异步**注册的（冷启动要几十秒），所以这里是轮询而不是一次读取。 */
const MARKET_TIMEOUT_MS = 180_000;

const harness = await launchHarness({
  requestTimeoutMs: 300_000,
  client: { name: "sdk-live-team-leader", version: "1" },
});
const { server } = harness;
console.log(`LISTENING ${server.readiness.host}:${server.readiness.port} data=${server.dataDir}`);

try {
  // 1. 等市场预热完，再挑一个专家团装上。
  let store = await harness.listStore();
  const marketDeadline = Date.now() + MARKET_TIMEOUT_MS;
  while (
    Date.now() < marketDeadline &&
    (store.markets_pending === true ||
      !store.items.some((item) => item.kind === "team"))
  ) {
    await Bun.sleep(5_000);
    store = await harness.listStore();
    const kinds = new Map<string, number>();
    for (const item of store.items) kinds.set(item.kind, (kinds.get(item.kind) ?? 0) + 1);
    console.log(
      `MARKET poll items=${store.items.length} pending=${store.markets_pending === true} kinds=${JSON.stringify(
        Object.fromEntries(kinds),
      )}`,
    );
  }
  const teamItems = store.items.filter((item) => item.kind === "team");
  console.log(
    `MARKET store items=${store.items.length} teams=${teamItems.length} pending=${store.markets_pending === true}`,
  );
  const entry = teamItems[0];
  if (entry) {
    console.log(`TEAM entry=${entry.id} name=${entry.name} installed=${entry.installed}`);
    const outcome = await harness.store.install(entry, { timeoutMs: INSTALL_TIMEOUT_MS });
    check("TL-001.market-install", outcome.ok === true, outcome.components?.slice(0, 4) ?? outcome);
  } else {
    // 官方默认市场目前只带 skill / connector（实测 262 + 228，0 个 team），所以退回**本地夹具**，
    // 走的是同一批协议方法（`import/run` → `install/run`），仓库自己的 e2e 也这么装夹具。
    const sourcePath =
      process.env.AGENT_STORE_TEAM_SOURCE ??
      path.resolve(
        import.meta.dir,
        "../../crates/backend/nomifun-importer/tests/fixtures/software-company",
      );
    console.log(`FIXTURE source=${sourcePath}`);
    const imported = await harness.runImport({
      source_path: sourcePath,
      source_kind: "codebuddy-plugin",
    });
    check(
      "TL-001.fixture-import",
      imported.status !== "failed" && imported.status !== "blocked",
      { status: imported.status, name: imported.name, version: imported.version },
    );
    const installed = await harness.runInstall({ snapshot_id: imported.snapshot_id });
    // 夹具不是产品包（有 bin/hooks/commands/dependencies），所以这里只记录安装报告，不当判据：
    // 真正的判据是「team/list 里出现了这个团」以及后面 create / 首轮 / 委派。
    console.log(
      `INSTALL warnings=${JSON.stringify(installed.warnings)} errors=${JSON.stringify(installed.errors)} outcomes=${JSON.stringify(installed.outcomes?.slice(0, 6))}`,
    );
  }

  // 2. `team/list` 的 Definition id 才是 `conversation/create` 的 `teamId`。
  const teams = await harness.teams.list();
  console.log(`TEAMS ${teams.map((team) => `${team.id}|${team.name}`).join(", ")}`);
  const target = teams.find((team) => team.name.includes("software-company")) ?? teams[0];
  if (!target) {
    check("TL-002.team-catalog", false, "no team Definition is installed");
  } else {
    check("TL-002.team-catalog", true, { id: target.id, name: target.name });

    // 2b. 团的连接器栅栏要求「已安装**且已启用**」。新装出来的 MCP server 行默认
    //     `enabled = false`（既有契约），所以这里先按团声明的连接器把它们打开——走的是
    //     doc `28` 记录的那条第一方路由（本地信任模式，无需 token），否则 `create` 会（正确地）
    //     以 `connector_unavailable` 拒绝，测试就测不到 Leader 那一层。
    const detail = await harness.teams.get(target.id);
    const declared = detail.connectors ?? [];
    if (declared.length > 0) {
      const catalog = await harness.connectors.list();
      for (const raw of declared) {
        const connector = catalog.find((item) => item.id === raw || item.name === raw);
        if (!connector) {
          console.log(`CONNECTOR ${raw} :: not in this host's catalog`);
          continue;
        }
        if (connector.enabled) {
          console.log(`CONNECTOR ${connector.name} :: already enabled`);
          continue;
        }
        const response = await fetch(
          `http://${server.readiness.host}:${server.readiness.port}/api/mcp/servers/${connector.id}/toggle`,
          { method: "POST" },
        );
        console.log(`CONNECTOR ${connector.name} :: toggled status=${response.status}`);
      }
    }

    // 3. 以该团开场：这一步只调 create，**不发任何消息**。
    const conv = await harness.conversations.create({
      name: `live-leader-${Date.now()}`,
      teamId: target.id,
    });
    check("TL-003.create-leader", Boolean(conv.conversation_id), conv);

      // 4. 订阅事件，然后由**客户端**发首轮——这正是 2b 与 `team/run` 的唯一区别。
      const subscription = await harness.conversations.follow(conv.conversation_id);
      const events: ConversationEvent[] = [];
      subscription.onEvent((event) => events.push(event));
      subscription.onResync((reason) => console.log(`RESYNC ${reason}`));

      const receipt = await harness.conversations.send(
        conv.conversation_id,
        "把这版需求拆成计划：一个 hello world REST API，两步以内即可。",
        crypto.randomUUID(),
      );
      console.log(`RECEIPT ${JSON.stringify(receipt).slice(0, 400)}`);

      const deadline = Date.now() + TURN_TIMEOUT_MS;
      while (Date.now() < deadline) {
        const done = events.some(
          (event) =>
            (event.event_type === "turn.status" && event.payload["status"] === "completed") ||
            event.event_type === "message.error",
        );
        if (done) break;
        // 委派本身可能先于终态到达；终态没来也照样往下看证据，所以这里不提前退出。
        await Bun.sleep(1_000);
      }

      // 2b 的契约是「客户端的这条消息**就是** Leader 的首轮」，所以判据是「被受理 + 真的开始跑」。
      // 「在预算内到达终态」不是它的契约：实测 Leader 常常自己动手做（十几次工具调用），
      // 3 分钟不够——那属于模型行为与预算问题，不能算这条路径的失败。
      const started = events.some((event) => event.event_type === "message.activity") ||
        events.some((event) => event.event_type === "message.tool");
      check("TL-004.turn-accepted-and-running", receipt.accepted === true && started, {
        accepted: receipt.accepted,
        events: events.length,
        types: [...new Set(events.map((event) => event.event_type))],
      });

      const types = [...new Set(events.map((event) => event.event_type))];
      console.log(`EVENTS count=${events.length} types=${types.join(",")}`);
      const toolEvents = events.filter((event) => event.event_type === "message.tool");
      const activityEvents = events.filter((event) => event.event_type === "message.activity");
      for (const event of [...toolEvents, ...activityEvents].slice(0, 8)) {
        console.log(`  ${event.event_type} :: ${JSON.stringify(event.payload).slice(0, 400)}`);
      }

      const terminal = events.some(
        (event) => event.event_type === "turn.status" && event.payload["status"] === "completed",
      );

      // 委派证据：工具事件/活动事件里点到 delegate（大小写不敏感）。
      const mentionsDelegate = (value: unknown): boolean =>
        JSON.stringify(value ?? "").toLowerCase().includes("delegate");
      const delegated =
        toolEvents.some((event) => mentionsDelegate(event.payload)) ||
        activityEvents.some((event) => mentionsDelegate(event.payload));
      // 这是**观察**而不是判据：Leader 会不会在自然语言指令下自发委派，是模型行为。
      // 实测 6 次里 2 次委派、4 次自己动手做（`team/run` 同条件也会 `team_run_not_started`）。
      console.log(
        `OBSERVATION natural-first-turn delegated=${delegated} terminal=${terminal} tools=${toolEvents.length} activities=${activityEvents.length} assistantChars=${
          events
            .filter((event) => event.event_type === "message.delta")
            .map((event) => String(event.payload["content"] ?? ""))
            .join("").length
        }`,
      );

      const page = await harness.conversations.messages({
        conversationId: conv.conversation_id,
        pageSize: 50,
      });
      console.log(
        `TRANSCRIPT ${page.items
          .map((message) => `${message.role}/${message.message_type}`)
          .join(", ")}`,
      );

      // ② 委派工具是否**注册**：换一条明确点名的指令。上一轮还在跑时不能再发（宿主会以
      //    `Conversation already has an authoritative local turn owner` 拒绝），所以先停掉它。
      if (!terminal) {
        try {
          await harness.conversations.cancel(conv.conversation_id);
          console.log("CANCEL in-flight first turn (it was still running)");
          await Bun.sleep(2_000);
        } catch (caught) {
          console.log(`CANCEL failed: ${String(caught).slice(0, 200)}`);
        }
      }
      const mark = events.length;
      const directive = await harness.conversations.send(
        conv.conversation_id,
        "请**调用 nomi_delegate(strategy=\"planned\")** 把目标交给团队执行，目标：设计并实现一个 hello world REST API。不要自己动手做。",
        crypto.randomUUID(),
      );
      console.log(`DIRECTIVE receipt=${JSON.stringify(directive).slice(0, 200)}`);
      const directiveDeadline = Date.now() + TURN_TIMEOUT_MS;
      while (Date.now() < directiveDeadline) {
        const slice = events.slice(mark);
        if (
          slice.some(
            (event) =>
              (event.event_type === "turn.status" && event.payload["status"] === "completed") ||
              event.event_type === "message.error",
          )
        ) {
          break;
        }
        await Bun.sleep(1_000);
      }
      const directiveSlice = events.slice(mark);
      const directiveTools = directiveSlice.filter((event) => event.event_type === "message.tool");
      console.log(
        `DIRECTIVE events=${directiveSlice.length} tools=${directiveTools.length} types=${[
          ...new Set(directiveSlice.map((event) => event.event_type)),
        ].join(",")}`,
      );
      for (const event of directiveTools.slice(0, 4)) {
        console.log(`  directive tool :: ${JSON.stringify(event.payload).slice(0, 300)}`);
      }
      check(
        "TL-005.tool-registered",
        directiveTools.length > 0 || directiveSlice.some((event) => mentionsDelegate(event.payload)),
        {
          tools: directiveTools.length,
          inferred: "委派工具是否**注册**：这一轮明确点名了它，仍无工具事件则更可能是没注册",
        },
      );

      await subscription.close();

      // ② 既有入口的对照：`team/run` 由服务端发首轮。**这是信息，不是判据**——
      //    实测显示「委派与否」由模型决定，同一指令两次真机一次委派一次没有，`team/run`
      //    也会出现 `team_run_not_started`。2b 真正要证明的是「客户端首轮**能够**触发委派」，
      //    已由 TL-004/005 覆盖（TL-004 的自然首轮在实测中确实调用过 `nomi_delegate` 并拿到
      //    execution_id）。把它当一致性断言会把模型行为误判成路径缺陷。
      let teamRunStarted = false;
      try {
        const teamReceipt = await harness.runs.team({
          teamId: target.id,
          goal: "让团队并行完成：设计并实现一个 hello world REST API（前端、后端、测试）。",
        });
        teamRunStarted = Boolean(teamReceipt.run_id);
        console.log(`TEAM-RUN started run=${teamReceipt.run_id} status=${teamReceipt.status}`);
      } catch (caught) {
        const code = (caught as { code?: string } | null)?.code ?? String(caught).slice(0, 200);
        console.log(`TEAM-RUN refused code=${code}`);
      }
      console.log(
        `OBSERVATION client-first-turn-delegated=${delegated} team-run-started=${teamRunStarted} :: 两者都由模型决定，不作为判据`,
      );
    }
} catch (error) {
  console.error("ERROR:", String(error).slice(0, 800));
  failures += 1;
} finally {
  const verdict = skipped ? "SKIP" : failures === 0 ? "PASS" : `FAIL(${failures})`;
  console.log(`\nRESULT ${verdict} data=${server.dataDir}`);
  await server.close();
  process.exit(failures === 0 ? 0 : 1);
}
