/**
 * doc `51` 真机验证：**市场图标在安装期固化进快照，导出面把它交回消费方**。
 *
 * 单测（`icon.rs` 的落位与三闸门、`app_server_expert_export.rs` 的 ref 填充、
 * `export.test.ts` 的落盘）证明的是内存夹具；这个脚本只证明真宿主能证明的四件事：
 *   ① 市场 `icons/<base>.<ext>` 与 entry 目录是**兄弟关系**，`copy_tree` 带不到它，
 *      只有安装期那一步能把字节写进不可变快照；
 *   ② 作者声明（`plugin.json.avatar`）优先，市场图标只补空缺且**绝不覆盖**；
 *   ③ 技能自带 `assets/icon.<ext>` 时，导出面第一次能**指名**它是图标
 *      （`skills[].icon`），字节仍走 `skill/file`；
 *   ④ 导出目录里拿到的图标字节与市场里那张逐字节相同。
 *
 * 用法（二进制必须含本次改动：`cargo build -p nomifun-app --bin agent-store`）：
 *   AGENT_STORE_BIN=.../target/debug/agent-store.exe bun scripts/verify-export-icons.ts
 *
 * 判据（PASS 需要下列全过）：
 *   EXI-001 无 `plugin.json.avatar` 的专家：`pack.avatar == avatars/<icon-basename>`，
 *           且该路径出现在 `pack.assets` 里，字节与市场图标相同
 *   EXI-002 有 `plugin.json.avatar` 的专家：`pack.avatar` 是作者那张，市场图标未落位
 *   EXI-003 专家团的包级技能自带 `assets/icon.png`：`skills[].icon == "assets/icon.png"`，
 *           `skill/file` 能读到同一份字节，导出目录里它在
 *           `skills/<name>/assets/icon.png`
 *   EXI-004 连接器图标的**可达性边界**（doc `51` §3.3.5）：包里 `mcpServers` 声明的
 *           连接器没有自己的图标来源，`connectors[].icon` 缺席——这条断言把"边界"
 *           钉住，任何一个方向上的语义变化都会让它变红
 *   EXI-005 导出目录里的三张图与市场/快照里的字节逐字节相同（`expert-pack.json`
 *           原样携带，不含任何宿主 URL）
 */

import { mkdtemp, mkdir, rm, writeFile, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { launchHarness, exportTeam } from "@flowy-agent-store/sdk";

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

/** Deterministic, distinguishable bytes: a wrong file is obvious in the diff. */
const bytesOf = (label: string): Buffer => Buffer.from(`ICON-BYTES:${label}`, "utf8");

const ICONLESS_ICON = bytesOf("market-iconless-expert");
const MARKET_LOSS_ICON = bytesOf("market-authored-expert-MUST-BE-IGNORED");
const AUTHORED_ICON = bytesOf("author-owns-this");
const SKILL_ICON = bytesOf("skill-in-directory");
const TEAM_ICON = bytesOf("market-team");

const EXPERT_DESCRIPTION = "A demo expert for the export-icon live check";

async function writeFileEnsured(file: string, body: string | Buffer): Promise<void> {
  await mkdir(path.dirname(file), { recursive: true });
  await writeFile(file, body);
}

/**
 * Two plugin entries in one expert market: one whose icon can only come from the
 * market's `icons/`, and one that declares its own `plugin.json.avatar` while the
 * market *also* ships an icon for it (the author must win).
 */
async function writeExpertMarket(root: string): Promise<void> {
  await writeFileEnsured(
    path.join(root, ".codebuddy-plugin/marketplace.json"),
    JSON.stringify(
      {
        name: "Export Icon Experts",
        version: "1.0.0",
        plugins: [
          { name: "iconless-expert", source: "./plugins/iconless-expert" },
          { name: "authored-expert", source: "./plugins/authored-expert" },
        ],
      },
      null,
      2,
    ),
  );
  for (const entry of ["iconless-expert", "authored-expert"]) {
    await writeFileEnsured(
      path.join(root, `plugins/${entry}/agents/${entry}.md`),
      `---\nname: ${entry}\ndescription: ${EXPERT_DESCRIPTION}\n---\n\n# ${entry}\n\ndoc 51 活体夹具。\n`,
    );
  }
  await writeFileEnsured(
    path.join(root, "plugins/iconless-expert/.codebuddy-plugin/plugin.json"),
    JSON.stringify(
      {
        name: "iconless-expert",
        version: "1.0.0",
        description: EXPERT_DESCRIPTION,
        expertType: "agent",
        agents: ["./agents/iconless-expert.md"],
        plugin: "iconless-expert",
        // No `avatar` on purpose: the only icon source is the market's `icons/`.
      },
      null,
      2,
    ),
  );
  await writeFileEnsured(
    path.join(root, "plugins/authored-expert/.codebuddy-plugin/plugin.json"),
    JSON.stringify(
      {
        name: "authored-expert",
        version: "1.0.0",
        description: EXPERT_DESCRIPTION,
        expertType: "agent",
        agents: ["./agents/authored-expert.md"],
        plugin: "authored-expert",
        avatar: "avatars/own.png",
      },
      null,
      2,
    ),
  );
  await writeFileEnsured(path.join(root, "plugins/authored-expert/avatars/own.png"), AUTHORED_ICON);
  // The market ships an icon for this entry too — it must lose (INV-4).
  await writeFileEnsured(path.join(root, "icons/authored-expert.png"), MARKET_LOSS_ICON);
  // …and this is the only icon the other entry can get.
  await writeFileEnsured(path.join(root, "icons/iconless-expert.png"), ICONLESS_ICON);
}

/**
 * One team package that ships a Skill carrying its own `assets/icon.png` and a
 * plugin-level `mcpServers` connector, plus a market icon for the **package**.
 */
async function writeTeamMarket(root: string): Promise<void> {
  await writeFileEnsured(
    path.join(root, ".codebuddy-plugin/marketplace.json"),
    JSON.stringify(
      {
        name: "Export Icon Teams",
        version: "1.0.0",
        plugins: [{ name: "demo-team", source: "./plugins/demo-team" }],
      },
      null,
      2,
    ),
  );
  await writeFileEnsured(
    path.join(root, "plugins/demo-team/.codebuddy-plugin/plugin.json"),
    JSON.stringify(
      {
        name: "demo-team",
        version: "1.0.0",
        description: "A demo team for the export-icon live check",
        expertType: "team",
        agentName: "demo-lead",
        teamInfo: { leadAgent: "demo-lead", memberAgents: ["demo-dev"] },
        agents: ["./agents/demo-lead.md", "./agents/demo-dev.md"],
        skills: ["./skills/demo-playbook"],
        mcpServers: {
          "demo-notes": { command: "npx", args: ["-y", "demo-notes-mcp"] },
        },
        plugin: "demo-team",
      },
      null,
      2,
    ),
  );
  for (const [file, name] of [
    ["demo-lead.md", "demo-lead"],
    ["demo-dev.md", "demo-dev"],
  ] as const) {
    await writeFileEnsured(
      path.join(root, "plugins/demo-team/agents", file),
      `---\nname: ${name}\ndescription: ${name}\n---\n\n# ${name}\n\ndoc 51 活体夹具。\n`,
    );
  }
  await writeFileEnsured(
    path.join(root, "plugins/demo-team/skills/demo-playbook/SKILL.md"),
    "---\nname: demo-playbook\ndescription: A demo skill that ships its own icon\n---\n\n# demo-playbook\n\ndoc 51 活体夹具。\n",
  );
  // The convention `SKILL_ICON_CANDIDATES` already defines — nothing new.
  await writeFileEnsured(
    path.join(root, "plugins/demo-team/skills/demo-playbook/assets/icon.png"),
    SKILL_ICON,
  );
  await writeFileEnsured(path.join(root, "icons/demo-team.png"), TEAM_ICON);
}

function sameBytes(actual: Uint8Array, expected: Uint8Array): boolean {
  if (actual.byteLength !== expected.byteLength) return false;
  return actual.every((byte, index) => byte === expected[index]);
}

const workDir = await mkdtemp(path.join(tmpdir(), "export-icons-"));
try {
  const expertMarket = path.join(workDir, "experts-market");
  const teamMarket = path.join(workDir, "teams-market");
  await writeExpertMarket(expertMarket);
  await writeTeamMarket(teamMarket);

  const harness = await launchHarness({
    bin: BIN,
    client: { name: "export-icons", version: "1.0.0" },
    marketDownload: "none",
    dataDir: path.join(workDir, "data"),
  });

  try {
    await harness.addMarketplace({
      name: "export-icons-experts",
      source_kind: "directory",
      source: expertMarket,
    });
    await harness.addMarketplace({
      name: "export-icons-teams",
      source_kind: "directory",
      source: teamMarket,
    });

    const items = await harness.store.list();
    const install = async (entry: string) => {
      const item = items.find((candidate) => candidate.entry_name === entry);
      if (!item) throw new Error(`store item ${entry} missing from the catalogue`);
      await harness.store.install(item);
    };
    for (const entry of ["iconless-expert", "authored-expert", "demo-team"]) {
      await install(entry);
    }

    const agents = await harness.agents.list();
    const teams = await harness.teams.list();
    const iconless = agents.find((agent) => agent.name === "iconless-expert");
    const authored = agents.find((agent) => agent.name === "authored-expert");
    const team = teams.find((candidate) => candidate.name === "demo-team");
    if (!iconless || !authored || !team) {
      throw new Error(
        `catalogue incomplete: agents=${agents.map((a) => a.name).join(",")} teams=${teams
          .map((t) => t.name)
          .join(",")}`,
      );
    }

    // --- EXI-001: the market icon is the only source, and it lands ----------
    const iconlessPack = await harness.agents.export(iconless.id);
    const iconlessAsset = iconlessPack.assets.find((asset) => asset.path === "avatars/iconless-expert.png");
    check(
      "EXI-001",
      iconlessPack.avatar === "avatars/iconless-expert.png",
      `pack.avatar=${iconlessPack.avatar ?? "<absent>"}`,
    );
    check(
      "EXI-001",
      iconlessAsset !== undefined,
      `listed in pack.assets: ${iconlessAsset ? `${iconlessAsset.size} B` : "NO"}`,
    );
    const iconlessBytes = await harness.readImportAsset(
      iconlessPack.provenance.snapshot_id,
      "avatars/iconless-expert.png",
    );
    check(
      "EXI-001",
      sameBytes(iconlessBytes, ICONLESS_ICON),
      "import/asset returns the market icon byte-for-byte",
    );

    // --- EXI-002: the author's declaration wins -----------------------------
    const authoredPack = await harness.agents.export(authored.id);
    check(
      "EXI-002",
      authoredPack.avatar === "avatars/own.png",
      `pack.avatar=${authoredPack.avatar ?? "<absent>"} (author's own)`,
    );
    check(
      "EXI-002",
      authoredPack.assets.every((asset) => asset.path !== "avatars/authored-expert.png"),
      "the market icon was not placed at all (INV-4: never overwrite)",
    );

    // --- EXI-003: a package's own skill icon is finally nameable -----------
    const teamPack = await harness.teams.export(team.id);
    const skillRef = teamPack.skills[0];
    check(
      "EXI-003",
      skillRef?.icon === "assets/icon.png",
      `skills[0].icon=${skillRef?.icon ?? "<absent>"}`,
    );
    if (skillRef) {
      const listing = await harness.skills.files(skillRef.id);
      check(
        "EXI-003",
        listing.files.some((file) => file.path === "assets/icon.png"),
        `skill/files lists it (${listing.files.length} files, truncated=${listing.truncated})`,
      );
      const skillIconBytes = await harness.skills.readFile(skillRef.id, "assets/icon.png");
      check(
        "EXI-003",
        sameBytes(skillIconBytes, SKILL_ICON),
        "skill/file returns the same bytes the package shipped",
      );
    }

    // --- EXI-004: the connector boundary, pinned ---------------------------
    check(
      "EXI-004",
      teamPack.connectors.length === 1,
      `the package's mcpServers produced ${teamPack.connectors.length} connector ref(s)`,
    );
    check(
      "EXI-004",
      teamPack.connectors.every((connector) => connector.icon == null),
      "connectors[].icon absent: a package icon is not a connector's icon (doc 51 §3.3.5)",
    );

    // --- EXI-005: materialise and diff the bytes --------------------------
    const dir = path.join(workDir, "exported-team");
    const result = await exportTeam(harness, team.id, dir);
    const onDisk = await readFile(path.join(dir, "skills", skillRef!.name, "assets/icon.png"));
    check(
      "EXI-005",
      sameBytes(onDisk, SKILL_ICON),
      `materialised at skills/${skillRef!.name}/assets/icon.png`,
    );
    const packJson = await readFile(path.join(dir, "expert-pack.json"), "utf8");
    check(
      "EXI-005",
      packJson === JSON.stringify(result.pack, null, 2),
      "expert-pack.json is the wire pack verbatim",
    );
    check(
      "EXI-005",
      !packJson.includes("/api/"),
      "no host URL travelled into the pack (doc 32 §2)",
    );
  } finally {
    await harness.close();
  }
} finally {
  await rm(workDir, { recursive: true, force: true });
}

if (failures > 0) {
  console.error(`FAIL doc-51 export icons: ${failures} check(s) failed`);
  process.exit(1);
}
console.log("PASS doc-51 export icons");
