/**
 * doc `50` 真机验证：**四类实体的四个展示字段落到 list 与 get 两面**。
 *
 * 单测（`manifest.rs` 的合并语义、`app_server_display.rs` 的解析、`team_summary`）
 * 证明的是内存夹具；这个脚本要证明**只有真宿主能证明**的四件事：
 *   ① 市场索引行的 `name_zh` / `description_zh` / `examples_zh` / `tags_zh` 真的经过
 *      导入期戳进 payload、再由 catalog 读回（技能与连接器两条不同的索引文件）；
 *   ② 作者声明（`plugin.json` 的 `displayName` / `quickPrompts`）仍然优先；
 *   ③ 团队用**兄弟键**写法（`description_zh` / `description_en`）时逐语种合并生效，
 *      而且包里那个语言不可靠的顶层 `description` 不被采用；
 *   ④ list 与 get 两面拿到的是同一份值（get 经 `flatten` 继承 summary）。
 *
 * 用法（二进制必须含本次改动：`cargo build -p agent-store`）：
 *   AGENT_STORE_BIN=.../target/debug/agent-store.exe bun scripts/verify-localized-display.ts
 *
 * 判据（PASS 需要下列全过）：
 *   LDF-001 连接器 list：`display_name.zh` / `display_description.zh` / `tags[0].zh` /
 *           `quick_prompts[0].zh` 全部来自索引行，而 `name` 仍是运行名 `demo-netdisk`
 *   LDF-002 连接器 get：与 list 相同
 *   LDF-003 技能 list：`display_description.zh` / `quick_prompts[0].zh` 来自索引行
 *   LDF-004 技能 get：与 list 相同
 *   LDF-005 专家 list：四个字段来自 `plugin.json`（作者声明优先）
 *   LDF-006 专家 get：与 list 相同
 *   LDF-007 团队 list：`display_name` 来自 `displayName`，`display_description.zh` 来自
 *           `description_zh` 兄弟键（顶层 `description` 是英文，不得被采用）
 *   LDF-008 团队 get：与 list 相同
 *   LDF-009 连接器 `credential` 块不受影响（`mode` / `status` 仍在）
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

const CONNECTOR = {
  zhName: "演示网盘",
  enName: "Demo Netdisk",
  zhDescription: "演示用连接器简介",
  enDescription: "A connector used by the live fixture",
  zhExample: "查看我演示网盘根目录下的文件",
  enExample: "List my demo disk",
  zhTag: "文件管理",
};

const SKILL = {
  zhName: "演示技能",
  zhDescription: "用来整理周报的演示技能",
  enDescription: "A demo skill that tidies weekly reports",
  zhExample: "用演示技能整理周报",
  zhTag: "办公效率",
};

const EXPERT = {
  zhName: "演示专家",
  enName: "Demo Expert",
  zhDescription: "演示专家的简介",
  enDescription: "The demo expert's description",
  zhPrompt: "帮我看下今天的动态",
};

const TEAM = {
  zhName: "演示专家团",
  enName: "Demo Team",
  zhDescription: "演示专家团的中文简介",
  enDescription: "The demo team's English description",
  zhPrompt: "帮我拆一下这个任务",
  /** Deliberately English, and deliberately *not* either declared locale. */
  misleadingBase: "English only, must never be picked up",
};

async function writeConnectorMarket(root: string): Promise<void> {
  await mkdir(path.join(root, ".codebuddy-connector"), { recursive: true });
  await mkdir(path.join(root, "connectors/demo-netdisk"), { recursive: true });
  await writeFile(
    path.join(root, ".codebuddy-connector/connectors.json"),
    JSON.stringify(
      {
        name: "Display Connectors",
        version: "1.0.0",
        connectors: [
          {
            id: "demo-netdisk",
            name: CONNECTOR.zhName,
            name_zh: CONNECTOR.zhName,
            name_en: CONNECTOR.enName,
            description: CONNECTOR.enDescription,
            description_zh: CONNECTOR.zhDescription,
            description_en: CONNECTOR.enDescription,
            source: "demo-netdisk",
            auth_mode: "",
            type: "mcp",
            examples_zh: [CONNECTOR.zhExample],
            examples_en: [CONNECTOR.enExample],
            tags_zh: [CONNECTOR.zhTag],
            tags_en: ["Files"],
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

async function writeSkillMarket(root: string): Promise<void> {
  await mkdir(path.join(root, ".codebuddy-skill"), { recursive: true });
  await mkdir(path.join(root, "skills/demo-notes"), { recursive: true });
  await writeFile(
    path.join(root, ".codebuddy-skill/marketplace.json"),
    JSON.stringify(
      {
        name: "Display Skills",
        version: "1.0.0",
        skills: [
          {
            name: "demo-notes",
            name_zh: SKILL.zhName,
            description: SKILL.enDescription,
            description_zh: SKILL.zhDescription,
            description_en: SKILL.enDescription,
            source: "demo-notes",
            examples_zh: [SKILL.zhExample],
            examples_en: ["Tidy my weekly report"],
            tags_zh: [SKILL.zhTag],
            tags_en: ["Productivity"],
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
    `---\nname: demo-notes\ndescription: ${SKILL.enDescription}\n---\n\n# 演示技能\n\ndoc 50 活体夹具。\n`,
    "utf8",
  );
}

/** An expert package: every display value is the author's own declaration. */
async function writeExpertMarket(root: string): Promise<void> {
  const entry = path.join(root, "plugins/demo-expert");
  await mkdir(path.join(root, ".codebuddy-plugin"), { recursive: true });
  await mkdir(path.join(entry, ".codebuddy-plugin"), { recursive: true });
  await mkdir(path.join(entry, "agents"), { recursive: true });
  await writeFile(
    path.join(root, ".codebuddy-plugin/marketplace.json"),
    JSON.stringify(
      {
        name: "Display Experts",
        version: "1.0.0",
        plugins: [{ name: "demo-expert", source: "./plugins/demo-expert" }],
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
        description: EXPERT.enDescription,
        agents: ["./agents/demo.md"],
        expertType: "agent",
        displayName: { zh: EXPERT.zhName, en: EXPERT.enName },
        displayDescription: { zh: EXPERT.zhDescription, en: EXPERT.enDescription },
        quickPrompts: [{ zh: EXPERT.zhPrompt, en: "Show today's updates" }],
        tags: [{ zh: "资讯", en: "News" }],
      },
      null,
      2,
    ),
    "utf8",
  );
  await writeFile(
    path.join(entry, "agents/demo.md"),
    `---\nname: demo-expert\ndescription: ${EXPERT.enDescription}\n---\n\n# 演示专家\n\ndoc 50 活体夹具。\n`,
    "utf8",
  );
}

/**
 * A team package: the display description arrives as the **sibling keys**
 * `description_zh` / `description_en` — the shape 18 of the 30 packages in the
 * real teams market use — while the top-level `description` is a third string
 * that must be ignored.
 */
async function writeTeamMarket(root: string): Promise<void> {
  const entry = path.join(root, "plugins/demo-team-pack");
  await mkdir(path.join(root, ".codebuddy-plugin"), { recursive: true });
  await mkdir(path.join(entry, ".codebuddy-plugin"), { recursive: true });
  await mkdir(path.join(entry, "agents"), { recursive: true });
  await writeFile(
    path.join(root, ".codebuddy-plugin/marketplace.json"),
    JSON.stringify(
      {
        name: "Display Teams",
        version: "1.0.0",
        plugins: [{ name: "demo-team-pack", source: "./plugins/demo-team-pack" }],
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
        name: "demo-team-pack",
        version: "1.0.0",
        description: TEAM.misleadingBase,
        description_zh: TEAM.zhDescription,
        description_en: TEAM.enDescription,
        agents: ["./agents/lead.md", "./agents/dev.md"],
        displayName: { zh: TEAM.zhName, en: TEAM.enName },
        quickPrompts: [{ zh: TEAM.zhPrompt, en: "Split this task" }],
        tags: [{ zh: "协作", en: "Collaboration" }],
        teamInfo: { leadAgent: "demo-lead", memberAgents: ["demo-dev"] },
      },
      null,
      2,
    ),
    "utf8",
  );
  for (const [file, name] of [
    ["lead.md", "demo-lead"],
    ["dev.md", "demo-dev"],
  ] as const) {
    await writeFile(
      path.join(entry, "agents", file),
      `---\nname: ${name}\ndescription: ${name}\n---\n\n# ${name}\n\ndoc 50 活体夹具。\n`,
      "utf8",
    );
  }
}

const workDir = await mkdtemp(path.join(tmpdir(), "localized-display-"));
try {
  const connectorMarket = path.join(workDir, "connectors-market");
  const skillMarket = path.join(workDir, "skills-market");
  const expertMarket = path.join(workDir, "experts-market");
  const teamMarket = path.join(workDir, "teams-market");
  await writeConnectorMarket(connectorMarket);
  await writeSkillMarket(skillMarket);
  await writeExpertMarket(expertMarket);
  await writeTeamMarket(teamMarket);

  const harness = await launchHarness({
    bin: BIN,
    client: { name: "localized-display", version: "1.0.0" },
    marketDownload: "none",
    dataDir: path.join(workDir, "data"),
  });

  try {
    for (const [name, dir] of [
      ["display-connectors", connectorMarket],
      ["display-skills", skillMarket],
      ["display-experts", expertMarket],
      ["display-teams", teamMarket],
    ] as const) {
      await harness.addMarketplace({ name, source_kind: "directory", source: dir });
    }

    const items = await harness.store.list();
    const byEntry = (entry: string) => items.find((item) => item.entry_name === entry);
    for (const entry of ["demo-netdisk", "demo-notes", "demo-expert", "demo-team-pack"]) {
      const item = byEntry(entry);
      if (!item) throw new Error(`store item ${entry} missing from the catalogue`);
      await harness.store.install(item);
    }

    // --- connector -------------------------------------------------------
    const connectors = await harness.connectors.list();
    const connector = connectors.find((entry) => entry.name === "demo-netdisk");
    check(
      "LDF-001",
      connector?.display_name?.zh === CONNECTOR.zhName &&
        connector?.display_name?.en === CONNECTOR.enName &&
        connector?.display_description?.zh === CONNECTOR.zhDescription &&
        connector?.tags?.[0]?.zh === CONNECTOR.zhTag &&
        connector?.quick_prompts?.[0]?.zh === CONNECTOR.zhExample &&
        connector?.name === "demo-netdisk",
      `list: display_name=${JSON.stringify(connector?.display_name ?? null)} name=${JSON.stringify(connector?.name ?? null)}`,
    );
    const connectorDetail = connector ? await harness.connectors.get(connector.id) : null;
    check(
      "LDF-002",
      connectorDetail?.display_name?.zh === CONNECTOR.zhName &&
        connectorDetail?.display_description?.zh === CONNECTOR.zhDescription &&
        connectorDetail?.quick_prompts?.[0]?.zh === CONNECTOR.zhExample &&
        connectorDetail?.tags?.[0]?.zh === CONNECTOR.zhTag,
      `get: display_description=${JSON.stringify(connectorDetail?.display_description ?? null)}`,
    );
    check(
      "LDF-009",
      connectorDetail?.credential?.mode === "none" && connectorDetail?.credential?.status === "not_required",
      `credential = ${JSON.stringify(connectorDetail?.credential ?? null)}`,
    );

    // --- skill -----------------------------------------------------------
    const skills = await harness.skills.list();
    const skill = skills.find((entry) => entry.name === "demo-notes");
    check(
      "LDF-003",
      skill?.display_name?.zh === SKILL.zhName &&
        skill?.display_description?.zh === SKILL.zhDescription &&
        skill?.display_description?.en === SKILL.enDescription &&
        skill?.quick_prompts?.[0]?.zh === SKILL.zhExample &&
        skill?.tags?.[0]?.zh === SKILL.zhTag,
      `list: display_description=${JSON.stringify(skill?.display_description ?? null)} quick_prompts=${JSON.stringify(skill?.quick_prompts ?? null)}`,
    );
    const skillDetail = skill ? await harness.skills.get(skill.id) : null;
    check(
      "LDF-004",
      skillDetail?.display_name?.zh === SKILL.zhName &&
        skillDetail?.display_description?.zh === SKILL.zhDescription &&
        skillDetail?.quick_prompts?.[0]?.zh === SKILL.zhExample,
      `get: display_name=${JSON.stringify(skillDetail?.display_name ?? null)}`,
    );

    // --- expert ----------------------------------------------------------
    const agents = await harness.agents.list();
    const agent = agents.find((entry) => entry.name === "demo-expert");
    check(
      "LDF-005",
      agent?.display_name?.zh === EXPERT.zhName &&
        agent?.display_description?.zh === EXPERT.zhDescription &&
        agent?.quick_prompts?.[0]?.zh === EXPERT.zhPrompt,
      `list: display_name=${JSON.stringify(agent?.display_name ?? null)} quick_prompts=${JSON.stringify(agent?.quick_prompts ?? null)}`,
    );
    const agentDetail = agent ? await harness.agents.get(agent.id) : null;
    check(
      "LDF-006",
      agentDetail?.display_name?.zh === EXPERT.zhName &&
        agentDetail?.display_description?.en === EXPERT.enDescription &&
        agentDetail?.quick_prompts?.[0]?.zh === EXPERT.zhPrompt,
      `get: display_description=${JSON.stringify(agentDetail?.display_description ?? null)}`,
    );

    // --- team ------------------------------------------------------------
    const teams = await harness.teams.list();
    const team = teams[0];
    check(
      "LDF-007",
      team?.display_name?.zh === TEAM.zhName &&
        team?.display_description?.zh === TEAM.zhDescription &&
        team?.display_description?.en === TEAM.enDescription &&
        team?.quick_prompts?.[0]?.zh === TEAM.zhPrompt &&
        team?.tags?.[0]?.zh === "协作",
      `list: display_description=${JSON.stringify(team?.display_description ?? null)} tags=${JSON.stringify(team?.tags ?? null)}`,
    );
    const teamDetail = team ? await harness.teams.get(team.id) : null;
    check(
      "LDF-008",
      teamDetail?.display_name?.zh === TEAM.zhName &&
        teamDetail?.display_description?.zh === TEAM.zhDescription &&
        teamDetail?.display_description?.en === TEAM.enDescription &&
        teamDetail?.quick_prompts?.[0]?.zh === TEAM.zhPrompt,
      `get: display_name=${JSON.stringify(teamDetail?.display_name ?? null)}`,
    );
  } finally {
    await harness.server.close();
  }
} finally {
  await rm(workDir, { recursive: true, force: true });
}

console.log(failures === 0 ? "\nPASS doc-50 localized display fields" : `\nFAIL ${failures} assertion(s)`);
process.exit(failures === 0 ? 0 : 1);
