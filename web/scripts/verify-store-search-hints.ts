/**
 * doc `49` 真机验证：**使用提示（提示词示例）落到目录面** + **检索口径**。
 *
 * 单测能证明的部分（`store-search.test.ts` 的字段表/区间、`app_server_store.rs`
 * 的索引行配对与投影回落）都是内存夹具；这个脚本要证明**只有真宿主能证明**的三件事：
 *   ① 索引行的 `examples_zh` 真的经过导入期/投影期到达 `StoreItem.quick_prompts`
 *      （连接器与技能两条不同的索引文件、两条不同的 kind 推导）；
 *   ② 专家的 `quick_prompts` 仍来自 `plugin.json`，不被索引行逻辑误伤；
 *   ③ 端到端 `search` 的多词 AND、高亮区间与「示例不参与检索」在真实目录上也成立。
 *
 * 用法（二进制必须含本次改动：`cargo build -p agent-store`）：
 *   AGENT_STORE_BIN=.../target/debug/agent-store.exe bun scripts/verify-store-search-hints.ts
 *
 * 判据（PASS 需要下列全过）：
 *   HSH-001 连接器条目 `quick_prompts` == 索引行 `examples_zh`（宿主回落生效）
 *   HSH-002 技能条目   `quick_prompts` == 索引行 `examples_zh`（同一条回落，另一份索引）
 *   HSH-003 专家条目   `quick_prompts` == `plugin.json` 的 `quickPrompts`（清单来源不变）
 *   HSH-004 多词查询 `演示 网盘` 命中连接器（旧实现整串子串为 0 命中）
 *   HSH-005 `searchWithHits` 的区间落在**原文**对应片段上（可按区间切片核对）
 *   HSH-006 只用示例文案里出现的词搜索 ⇒ 0 命中（D9 / INV-5）
 *   HSH-007 `listStatus().marketsPending` 是布尔值（`marketDownload: none` 下为 false）
 */

import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { launchHarness } from "@flowy-agent-store/sdk";

const BIN = process.env["AGENT_STORE_BIN"] ?? "C:\\workspace\\allo\\target\\debug\\agent-store.exe";

let failures = 0;
function check(id: string, ok: boolean, detail: string): void {
  if (ok) {
    console.log(`  ✓ ${id} ${detail}`);
    return;
  }
  failures += 1;
  console.log(`  ✗ ${id} ${detail}`);
}

const CONNECTOR_EXAMPLES_ZH = [
  "查看我演示网盘根目录下的文件",
  "搜索与「项目合同」相关的文件",
];
const SKILL_EXAMPLES_ZH = ["用演示技能整理周报"];
const EXPERT_PROMPT_ZH = "帮我看下今天的动态";

/** A connector market: the index row carries `examples_*`, the entry has no plugin.json. */
async function writeConnectorMarket(root: string): Promise<void> {
  await mkdir(path.join(root, ".codebuddy-connector"), { recursive: true });
  await mkdir(path.join(root, "connectors/demo-netdisk/skills"), { recursive: true });
  await writeFile(
    path.join(root, ".codebuddy-connector/connectors.json"),
    JSON.stringify(
      {
        name: "Hints Connectors",
        version: "1.0.0",
        connectors: [
          {
            id: "demo-netdisk",
            name: "演示网盘",
            // Real connector rows carry `name_zh` too (doc `18` §4.1 census); the
            // projection prefers it for the displayed name, so the highlight
            // assertion below has a CJK label to point at.
            name_zh: "演示网盘",
            name_en: "Demo Netdisk",
            description: "演示用连接器",
            source: "demo-netdisk",
            auth_mode: "",
            type: "mcp",
            examples_zh: CONNECTOR_EXAMPLES_ZH,
            examples_en: ["List my demo disk", "Find files about the contract"],
          },
        ],
      },
      null,
      2,
    ),
    "utf8",
  );
  await writeFile(
    path.join(root, "connectors/demo-netdisk/mcp.json"),
    JSON.stringify(
      { mcpServers: { "demo-netdisk": { type: "sse", url: "https://example.invalid/sse" } } },
      null,
      2,
    ),
    "utf8",
  );
}

/** A skill market: same `examples_*` declaration, different index file and kind. */
async function writeSkillMarket(root: string): Promise<void> {
  await mkdir(path.join(root, ".codebuddy-skill"), { recursive: true });
  await mkdir(path.join(root, "skills/demo-notes"), { recursive: true });
  await writeFile(
    path.join(root, ".codebuddy-skill/marketplace.json"),
    JSON.stringify(
      {
        name: "Hints Skills",
        version: "1.0.0",
        skills: [
          {
            name: "demo-notes",
            description: "演示技能",
            source: "demo-notes",
            examples_zh: SKILL_EXAMPLES_ZH,
            examples_en: ["Tidy my weekly report"],
          },
        ],
      },
      null,
      2,
    ),
    "utf8",
  );
  await writeFile(
    path.join(root, "skills/demo-notes/SKILL.md"),
    "---\nname: demo-notes\ndescription: 演示技能\n---\n\n# 演示技能\n\ndoc 49 活体夹具。\n",
    "utf8",
  );
}

/** An expert market: the prompt hints live in `plugin.json`, not in an index row. */
async function writeExpertMarket(root: string): Promise<void> {
  const entry = path.join(root, "plugins/demo-expert");
  await mkdir(path.join(root, ".codebuddy-plugin"), { recursive: true });
  await mkdir(path.join(entry, ".codebuddy-plugin"), { recursive: true });
  await mkdir(path.join(entry, "agents"), { recursive: true });
  await writeFile(
    path.join(root, ".codebuddy-plugin/marketplace.json"),
    JSON.stringify(
      {
        name: "Hints Experts",
        version: "1.0.0",
        plugins: [{ name: "demo-expert", source: "./plugins/demo-expert", description: "演示专家" }],
      },
      null,
      2,
    ),
    "utf8",
  );
  await writeFile(
    path.join(entry, ".codebuddy-plugin/plugin.json"),
    JSON.stringify(
      {
        name: "demo-expert",
        version: "1.0.0",
        description: "演示专家",
        agents: ["./agents/demo.md"],
        expertType: "agent",
        displayName: { zh: "演示专家", en: "Demo Expert" },
        quickPrompts: [{ zh: EXPERT_PROMPT_ZH, en: "Show today's updates" }],
      },
      null,
      2,
    ),
    "utf8",
  );
  await writeFile(
    path.join(entry, "agents/demo.md"),
    "---\nname: demo-expert\ndescription: 演示专家\n---\n\n# 演示专家\n\ndoc 49 活体夹具。\n",
    "utf8",
  );
}

const workDir = await mkdtemp(path.join(tmpdir(), "store-search-hints-"));
try {
  const connectorMarket = path.join(workDir, "connectors-market");
  const skillMarket = path.join(workDir, "skills-market");
  const expertMarket = path.join(workDir, "experts-market");
  await writeConnectorMarket(connectorMarket);
  await writeSkillMarket(skillMarket);
  await writeExpertMarket(expertMarket);

  const harness = await launchHarness({
    bin: BIN,
    client: { name: "store-search-hints", version: "1.0.0" },
    // No default sources at all: the three fixtures below are the whole catalogue.
    marketDownload: "none",
    dataDir: path.join(workDir, "data"),
  });

  try {
    for (const [name, dir] of [
      // The marketplace id is derived from the name and the DB constrains it to
      // lowercase `[0-9a-z-]` — a title-case name is rejected at `market/add`.
      ["hints-connectors", connectorMarket],
      ["hints-skills", skillMarket],
      ["hints-experts", expertMarket],
    ] as const) {
      await harness.addMarketplace({ name, source_kind: "directory", source: dir });
    }

    const items = await harness.store.list();
    const byEntry = (entry: string) => items.find((item) => item.entry_name === entry);

    const connector = byEntry("demo-netdisk");
    check(
      "HSH-001",
      connector?.quick_prompts?.[0]?.zh === CONNECTOR_EXAMPLES_ZH[0] &&
        connector?.quick_prompts?.[1]?.en === "Find files about the contract",
      `连接器 quick_prompts = ${JSON.stringify(connector?.quick_prompts ?? null)}`,
    );

    const skill = byEntry("demo-notes");
    check(
      "HSH-002",
      skill?.quick_prompts?.[0]?.zh === SKILL_EXAMPLES_ZH[0],
      `技能 quick_prompts = ${JSON.stringify(skill?.quick_prompts ?? null)}`,
    );

    const expert = byEntry("demo-expert");
    check(
      "HSH-003",
      expert?.quick_prompts?.[0]?.zh === EXPERT_PROMPT_ZH,
      `专家 quick_prompts = ${JSON.stringify(expert?.quick_prompts ?? null)}`,
    );

    // HSH-004: two terms that sit inside one name, not contiguously.
    const multi = await harness.store.search("演示 网盘");
    check(
      "HSH-004",
      multi.length === 1 && multi[0]?.entry_name === "demo-netdisk",
      `search("演示 网盘") → ${JSON.stringify(multi.map((item) => item.entry_name))}`,
    );

    // HSH-005: the range must address the rendered string, not a folded copy.
    const [match] = await harness.store.searchWithHits("网盘");
    const nameHit = match?.hits.find((hit) => hit.field === "name");
    const sliced = nameHit ? nameHit.ranges.map(([start, end]) => match.item.name.slice(start, end)) : [];
    check(
      "HSH-005",
      sliced.length === 1 && sliced[0] === "网盘" && match?.item.entry_name === "demo-netdisk",
      `name 区间切片 = ${JSON.stringify(sliced)}（name = ${JSON.stringify(match?.item.name ?? null)}）`,
    );

    // HSH-006: an example's own words must not make an entry searchable.
    const byExample = await harness.store.search("项目合同");
    check(
      "HSH-006",
      byExample.length === 0,
      `search("项目合同") → ${byExample.length} 命中（应为 0：示例不参与检索）`,
    );

    const status = await harness.store.listStatus();
    check(
      "HSH-007",
      typeof status.marketsPending === "boolean" && status.items.length === items.length,
      `marketsPending = ${status.marketsPending}，items = ${status.items.length}`,
    );
  } finally {
    await harness.server.close();
  }
} finally {
  await rm(workDir, { recursive: true, force: true });
}

console.log(failures === 0 ? "\nPASS doc-49 search + prompt hints" : `\nFAIL ${failures} assertion(s)`);
process.exit(failures === 0 ? 0 : 1);
