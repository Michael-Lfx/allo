/** doc `32` 真机验证：`agent/export` / `team/export` 把专家 / 专家团导成一份**可移植定义**，
 * 外部 runtime 拿它自己跑。
 *
 * 为什么单独立一个脚本：单测用 fake 的 `ExpertPresetReader` + 内存 SQLite，只能证明「按 adapter 的
 * 读法读出来的东西是对的」。这个脚本要证明的是**端到端**那三件单测碰不到的事：
 *   ① persona 与夹具磁盘上 `agents/*.md` 的**正文逐字相等**（不是「看起来像」）；
 *   ② 技能是**引用**——包里的名字真能经 `skill/files` / `skill/file` 取回**逐字节相同**的内容；
 *   ③ §6.5 那段 11 行「pack → 目录」配方在真宿主上真的能跑出一个自洽目录。
 *
 * 用法（二进制必须含本次改动）：
 *   AGENT_STORE_BIN=.../target/debug/agent-store.exe bun scripts/sdk-live-expert-export.ts
 *
 * 判据（PASS 需要下列全过）：
 *   EX-001 能力位 `expert_export` 为真（宿主接了 seam）
 *   EX-002 夹具 `file-paths` 装上；`agents.get` 拿不到正文，`agent/export` 拿到**逐字相同**的正文
 *   EX-003 同一专家连续导出两次**逐字节相同**（无时间戳、列表有序）
 *   EX-004 技能是引用：`skills.files('hello')` 能列、`readFile` 与夹具字节相同
 *   EX-005 §6.5 的「pack → 目录」配方跑通：目录自洽（pack JSON / persona.md / skills/hello/SKILL.md 三者都可核对）
 *   EX-006 `file-paths` 的团：成员**团长在首位**，每个成员自带正文
 *   EX-007 `software-company` 的 5 人团：声明列表与展开列表都对
 *   EX-008 悬空引用**如实上报**（该夹具声明的 `planning` 在本机没有对应技能）——包报的是**声明**，
 *          能不能解析是宿主事实，消费方必须自己判
 *   EX-009 连接器按**安装态**：声明了但没启用 ⇒ 包为空；用第一方路由启用后 ⇒ 包里出现
 *   EX-010 doc `35` 的 team 写目录：`exportTeam` 写出 `members/<id>/persona.md`（无顶层 persona.md）、
 *          团**包级**技能与成员声明**两个来源**都落盘（去重、逐字节），悬空声明进 `danglingSkills`
 *   EX-011 官方团的真实形状：技能只在**包级**声明、成员一个都不写（镜像 `stock-partner-team` 1.0.7）
 *          ⇒ 顶层 `pack.skills` 非空、两个技能整个目录（含 `references/`）逐字节落盘
 */
import { mkdtemp, readdir, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { exportAgent, exportTeam, launchHarness } from "@flowy-agent-store/sdk";

let failures = 0;
function check(name: string, ok: boolean, detail?: unknown): void {
  const suffix = detail === undefined ? "" : ` :: ${JSON.stringify(detail).slice(0, 700)}`;
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${suffix}`);
  if (!ok) failures += 1;
}

const FIXTURES = path.resolve(
  import.meta.dir,
  "../../crates/backend/nomifun-importer/tests/fixtures",
);

/** The Agent Markdown body exactly as the importer defines it: everything after
 * the closing `---` of the frontmatter, minus the single newline that follows it
 * (`nomifun-importer/src/frontmatter.rs`). Anything else here would make the
 * fidelity assertion weaker than the one doc `32` §9 asks for. */
function markdownBody(markdown: string): string {
  const match = /^---\r?\n[\s\S]*?\r?\n---[ \t]*\r?\n?/.exec(markdown);
  return match ? markdown.slice(match[0].length) : markdown;
}

async function importAndInstall(
  harness: Awaited<ReturnType<typeof launchHarness>>,
  fixture: string,
): Promise<string> {
  const source = path.join(FIXTURES, fixture);
  const imported = await harness.runImport({ source_path: source, source_kind: "codebuddy-plugin" });
  const installed = await harness.runInstall({ snapshot_id: imported.snapshot_id });
  console.log(
    `INSTALL ${fixture} status=${imported.status} warnings=${installed.warnings?.length ?? 0} errors=${installed.errors?.length ?? 0} outcomes=${installed.outcomes?.length ?? 0}`,
  );
  return imported.snapshot_id;
}

const harness = await launchHarness({
  requestTimeoutMs: 300_000,
  client: { name: "sdk-live-expert-export", version: "1" },
});
const { server } = harness;
console.log(`LISTENING ${server.readiness.host}:${server.readiness.port} data=${server.dataDir}`);

let workdir: string | null = null;
try {
  check(
    "EX-001.capability",
    harness.initializeInfo?.capabilities.expert_export === true,
    harness.initializeInfo?.capabilities,
  );

  // ---------------------------------------------------------------- 专家
  await importAndInstall(harness, "file-paths");
  const agents = await harness.agents.list();
  console.log(`AGENTS ${agents.map((agent) => `${agent.id}|preset=${agent.preset_id ?? "-"}`).join(", ")}`);
  const lead = agents.find((agent) => agent.name === "lead-agent") ?? agents[0];
  if (!lead) throw new Error("the file-paths fixture produced no agent Definition");

  const detail = await harness.agents.get(lead.id);
  const detailKeys = Object.keys(detail);
  console.log(`DETAIL keys=${detailKeys.join(",")}`);

  const pack = await harness.agents.export(lead.id);
  const fixtureLead = await readFile(path.join(FIXTURES, "file-paths/agents/lead-agent.md"), "utf8");
  const expectedBody = markdownBody(fixtureLead);
  console.log(
    `PACK agent=${pack.id} format=${pack.pack_format} kind=${pack.kind} personaChars=${pack.persona.instructions.length} expectedChars=${expectedBody.length}`,
  );
  check(
    "EX-002.export-and-fidelity",
    pack.pack_format === 1 &&
      pack.kind === "agent" &&
      // The catalog face must have no field that could carry the body…
      !detailKeys.includes("instructions") &&
      !detailKeys.includes("persona") &&
      // …and the export must carry it verbatim.
      pack.persona.instructions === expectedBody,
    {
      catalogKeys: detailKeys.filter((key) => key.includes("instruct") || key.includes("persona")),
      personaChars: pack.persona.instructions.length,
      expectedChars: expectedBody.length,
      exact: pack.persona.instructions === expectedBody,
    },
  );

  const again = await harness.agents.export(lead.id);
  check(
    "EX-003.deterministic",
    JSON.stringify(pack) === JSON.stringify(again),
    { bytes: JSON.stringify(pack).length },
  );

  // ---------------------------------------------------------------- 技能（引用）
  const skillName = pack.skills[0]?.name;
  const inventory = skillName ? await harness.skills.files(skillName) : null;
  const viaWire =
    skillName && inventory ? await harness.skills.readFile(skillName, "SKILL.md") : null;
  const onDisk = skillName
    ? new Uint8Array(await readFile(path.join(FIXTURES, `file-paths/skills/${skillName}/SKILL.md`)))
    : null;
  console.log(
    `SKILLS pack=${JSON.stringify(pack.skills)} inventory=${inventory?.files.map((file) => file.path).join(",") ?? "-"} digest=${inventory?.content_digest.slice(0, 12) ?? "-"}`,
  );
  check(
    "EX-004.skills-are-references",
    Boolean(skillName && inventory && viaWire && onDisk) &&
      inventory!.files.some((file) => file.path === "SKILL.md") &&
      viaWire!.length === onDisk!.length &&
      viaWire!.every((byte, index) => byte === onDisk![index]),
    {
      skill: skillName,
      files: inventory?.files.length,
      bytes: viaWire?.length,
      onDiskBytes: onDisk?.length,
    },
  );

  // ---------------------------------------------------------------- §6.5 的配方
  // doc `35` 把 §6.5 的 11 行配方提升成了 SDK 公开面 `exportAgent` —— EX-005
  // 现在调的是新 API，判据不变：目录自洽（pack JSON / persona.md /
  // skills/hello/SKILL.md 三者都可核对，且与线上 pack / 夹具字节相同）。
  workdir = await mkdtemp(path.join(tmpdir(), "expert-export-"));
  const materialized = path.join(workdir, pack.id);
  const exported = await exportAgent(harness, pack.id, materialized);
  // The live fixture declares one resolvable skill; a dangling one is expected
  // only in the big-team section (EX-008), so this section must stay clean.
  check(
    "EX-005a.export-agent-no-dangling",
    exported.pack.id === pack.id && exported.danglingSkills.length === 0,
    { packId: exported.pack.id, dangling: exported.danglingSkills },
  );
  const packJsonOnDisk = await readFile(path.join(materialized, "expert-pack.json"), "utf8");
  const personaOnDisk = await readFile(path.join(materialized, "persona.md"), "utf8");
  const materializedSkill =
    skillName
      ? new Uint8Array(
          await readFile(path.join(materialized, "skills", skillName, "SKILL.md")),
        )
      : null;
  console.log(
    `MATERIALIZE dir=${materialized} skills=${JSON.stringify(exported.writtenSkills)} dangling=${JSON.stringify(exported.danglingSkills)}`,
  );
  check(
    "EX-005.materialize-recipe",
    packJsonOnDisk === JSON.stringify(pack, null, 2) &&
      personaOnDisk === pack.persona.instructions &&
      materializedSkill !== null &&
      onDisk !== null &&
      materializedSkill.length === onDisk.length &&
      materializedSkill.every((byte, index) => byte === onDisk[index]),
    {
      packJsonBytes: packJsonOnDisk.length,
      personaChars: personaOnDisk.length,
      writtenSkills: exported.writtenSkills.length,
      bytesEqual: materializedSkill?.length === onDisk?.length,
    },
  );

  // ---------------------------------------------------------------- 团
  const teams = await harness.teams.list();
  console.log(`TEAMS ${teams.map((team) => `${team.id}|${team.name}|members=${team.member_agent_ids.length}`).join(", ")}`);
  const small = teams.find((team) => team.member_agent_ids.length > 0 && team.member_agent_ids.length < 3);
  if (small) {
    const teamPack = await harness.teams.export(small.id);
    const members = teamPack.team?.members ?? [];
    const ids = members.map((member) => member.id);
    const leadId = members[0]?.id;
    console.log(
      `TEAMPACK id=${teamPack.id} kind=${teamPack.kind} declared=${JSON.stringify(teamPack.team?.member_agent_ids)} expanded=${JSON.stringify(ids)} personas=${members.map((member) => member.persona.instructions.length).join("/")}`,
    );
    check(
      "EX-006.team-roster-ordered",
      teamPack.kind === "team" &&
        members.length >= 2 &&
        leadId === small.lead_agent_id &&
        ids[0] === small.lead_agent_id &&
        members.every((member) => member.persona.instructions.length > 0) &&
        // The team's own Preset is not part of the definition. Note `?? null`:
        // an absent optional is **omitted** from the JSON
        // (`skip_serializing_if`), so it arrives as `undefined`, not `null`.
        (teamPack.provenance.preset_revision ?? null) === null &&
        (teamPack.model.resolved ?? null) === null,
      {
        declared: teamPack.team?.member_agent_ids,
        expanded: ids,
        leadMatches: leadId === small.lead_agent_id,
        presetRevision: teamPack.provenance.preset_revision ?? null,
        resolvedModel: teamPack.model.resolved ?? null,
      },
    );
  } else {
    check("EX-006.team-roster-ordered", false, "no small team Definition was installed");
  }

  // ------------------------------------------------- 5 人团 + 悬空引用观测
  const bigSnapshot = await importAndInstall(harness, "software-company");
  const teams2 = await harness.teams.list();
  const big = teams2.find((team) => team.member_agent_ids.length >= 4);
  if (big) {
    const bigPack = await harness.teams.export(big.id);
    const members = bigPack.team?.members ?? [];
    const ids = members.map((member) => member.id);
    console.log(
      `BIGTEAM declared=${JSON.stringify(bigPack.team?.member_agent_ids)} expanded=${JSON.stringify(ids)} lead=${bigPack.team?.lead_agent_id}`,
    );
    check(
      "EX-007.big-roster",
      ids.length === big.member_agent_ids.length + 1 &&
        ids[0] === bigPack.team?.lead_agent_id,
      { declared: big.member_agent_ids.length, expanded: ids.length },
    );

    // 悬空引用：夹具声明 `planning` / `coding` / `requirements`，本机一个都没有。
    const installed = new Set((await harness.skills.list()).map((skill) => skill.name));
    const declared = members.flatMap((member) => member.skills.map((skill) => skill.name));
    const unresolvable = declared.filter((name) => !installed.has(name));
    console.log(
      `DANGLING declared=${JSON.stringify(declared)} installedSkills=${JSON.stringify([...installed])} unresolvable=${JSON.stringify(unresolvable)}`,
    );
    check(
      "EX-008.dangling-references-reported",
      // The pack reports the **declaration**; resolvability is a host fact the
      // consumer has to check (doc `32` §5 R8). Reporting them is the honest
      // behaviour — silently dropping them would hide a requirement.
      unresolvable.length === declared.length && declared.length > 0,
      { declared: declared.length, unresolvable: unresolvable.length, installed: [...installed] },
    );

    // ------------------------------------------------- 连接器按安装态
    // `team/get` publishes the same projection the pack must agree with: both
    // read the installer's state, not the manifest. Moving the state through the
    // public wire (`install/disable` / `install/enable`) is what proves the pack
    // *follows* it rather than reporting a cached or declared list.
    const detailTeam = await harness.teams.get(big.id);
    const declaredConnectors = detailTeam.connectors ?? [];
    const status = await harness.getInstallStatus(bigSnapshot);
    const connectorComponent = status.components.find((component) => component.kind === "connector");
    const packConnectors = bigPack.connectors.map((connector) => connector.id);
    console.log(
      `CONNECTORS declared=${JSON.stringify(declaredConnectors)} pack=${JSON.stringify(packConnectors)} component=${connectorComponent?.id ?? "-"}`,
    );
    let whileDisabled: string[] = [];
    if (connectorComponent) {
      await harness.disableInstall(bigSnapshot, [connectorComponent.id]);
      whileDisabled = (await harness.teams.export(big.id)).connectors.map((c) => c.id);
      await harness.enableInstall(bigSnapshot, [connectorComponent.id]);
    }
    const restored = (await harness.teams.export(big.id)).connectors.map((c) => c.id);
    console.log(
      `CONNECTORS whileDisabled=${JSON.stringify(whileDisabled)} restored=${JSON.stringify(restored)}`,
    );
    check(
      "EX-009.connectors-follow-install-state",
      Boolean(connectorComponent) &&
        declaredConnectors.length > 0 &&
        packConnectors.join(",") === declaredConnectors.join(",") &&
        whileDisabled.length === 0 &&
        restored.join(",") === declaredConnectors.join(","),
      { declaredConnectors, packConnectors, whileDisabled, restored },
    );

    // ------------------------------------------------- EX-010：team 写目录（doc `35` §8.2）
    // `exportTeam` = `team/export` + 成员 persona + 技能字节落盘。团的技能有**两个来源**：
    // ① 团**包级**声明的技能（`plugin.json.skills` ⇒ 快照里自己的 skill 组件，装机时已被
    //    物化到托管技能根目录）；② 各成员 frontmatter 里的 `skills:` 声明。两个来源都不合并在
    //    服务端，`materializePack` 才把它们并起来去重。
    // 本夹具里 ① 是 `release-notes` / `review-checklist`（包自带、装机后一定能解析），
    // ② 是 `planning` / `requirements` / `coding`（本机一个都没有 ⇒ 悬空必须如实上报）。
    const teamDir = path.join(workdir ?? ".", "ex010", big.id);
    const teamExport = await exportTeam(harness, big.id, teamDir);
    const teamFiles = await readdir(teamDir, { withFileTypes: true });
    const topPersona = teamFiles.some((entry) => entry.name === "persona.md");
    let memberPersonaFiles = 0;
    if (teamFiles.some((entry) => entry.isDirectory() && entry.name === "members")) {
      memberPersonaFiles = (await readdir(path.join(teamDir, "members"))).length;
    }
    const danglingIds = teamExport.danglingSkills.map((entry) => entry.id);
    const packageSkills = teamExport.pack.skills.map((skill) => skill.name);
    // 两个来源合起来，每个声明都要恰好落在 written 或 dangling 一侧，不多不少；
    // 两侧相交就是「既说写了又说取不到」的自相矛盾。
    const declaredAll = [...new Set([...packageSkills, ...declared])];
    const accounted = new Set([...teamExport.writtenSkills, ...danglingIds]);
    const everyDeclarationAccounted =
      declaredAll.every((name) => accounted.has(name)) &&
      danglingIds.every((id) => !teamExport.writtenSkills.includes(id));
    // 包级技能是「这个包自己带的」，装机后必然可解析 ⇒ 必须真的落盘，且逐字节相同。
    const releaseNotesOnDisk = new Uint8Array(
      await readFile(path.join(FIXTURES, "software-company/skills/release-notes/SKILL.md")),
    );
    const releaseNotesExported = new Uint8Array(
      await readFile(path.join(teamDir, "skills/release-notes/SKILL.md")),
    );
    console.log(
      `TEAMMATERIALIZE dir=${teamDir} members=${memberPersonaFiles} packageSkills=${JSON.stringify(packageSkills)} written=${JSON.stringify(teamExport.writtenSkills)} dangling=${JSON.stringify(danglingIds)} topPersona=${topPersona}`,
    );
    check(
      "EX-010.team-materialize",
      teamExport.pack.id === big.id &&
        !topPersona &&
        memberPersonaFiles === members.length &&
        // ① 包级技能进了包、落了盘、字节相同（修复前这里恒为空）
        packageSkills.length > 0 &&
        packageSkills.every((name) => teamExport.writtenSkills.includes(name)) &&
        releaseNotesExported.length === releaseNotesOnDisk.length &&
        releaseNotesExported.every((byte, index) => byte === releaseNotesOnDisk[index]) &&
        // ② 成员声明的悬空引用如实上报，且两侧不交
        danglingIds.length > 0 &&
        danglingIds.every((id) => unresolvable.includes(id)) &&
        everyDeclarationAccounted,
      {
        members: memberPersonaFiles,
        expectedMembers: members.length,
        packageSkills,
        written: teamExport.writtenSkills,
        dangling: danglingIds,
        unresolvable,
        everyDeclarationAccounted,
      },
    );
  } else {
    check("EX-007.big-roster", false, "software-company produced no multi-member team");
    check("EX-008.dangling-references-reported", false, "skipped: no big team");
    check("EX-009.connectors-follow-install-state", false, "skipped: no big team");
    check("EX-010.team-materialize", false, "skipped: no big team");
  }

  // ------------------------------------------------- EX-011：官方团的真实形状
  // 上面那个夹具是手写的，成员 frontmatter 里带着 `skills:` 声明 —— 这恰恰**不是**官方团的形状。
  // 真机实测 `stock-partner-team` 1.0.7：包级声明 3 个技能、7 个成员、成员声明 0 个；
  // 修复前这条路导出的是「顶层恒空 + 成员全空」= 0 个技能。这个夹具镜像那个形状，
  // 断言技能只可能来自包级、且整个技能目录（含 `references/`）逐字节落盘。
  await importAndInstall(harness, "package-skills-team");
  const shapedTeams = await harness.teams.list();
  const shaped = shapedTeams.find((team) => team.id.includes("package-skills-team"));
  if (shaped) {
    const shapedDir = path.join(workdir ?? ".", "ex011", shaped.id);
    const shapedExport = await exportTeam(harness, shaped.id, shapedDir);
    const packageSkills = shapedExport.pack.skills.map((skill) => skill.name).sort();
    const memberDeclarations = (shapedExport.pack.team?.members ?? []).flatMap((member) =>
      member.skills.map((skill) => skill.name),
    );
    const expected = ["alpha-playbook", "beta-playbook"];
    const written = [...shapedExport.writtenSkills].sort();
    const alphaOnDisk = new Uint8Array(
      await readFile(path.join(FIXTURES, "package-skills-team/skills/alpha-playbook/SKILL.md")),
    );
    const alphaExported = new Uint8Array(
      await readFile(path.join(shapedDir, "skills/alpha-playbook/SKILL.md")),
    );
    const stepsWire = await harness.skills.readFile("alpha-playbook", "references/steps.md");
    const stepsExported = new Uint8Array(
      await readFile(path.join(shapedDir, "skills/alpha-playbook/references/steps.md")),
    );
    console.log(
      `PACKAGESKILLS packageSkills=${JSON.stringify(packageSkills)} memberDeclarations=${JSON.stringify(memberDeclarations)} written=${JSON.stringify(written)}`,
    );
    check(
      "EX-011.package-level-team-skills",
      shapedExport.pack.id === shaped.id &&
        // 成员一个都没声明 ⇒ 写出来的技能只可能来自包级（这一条就是回归判据）
        memberDeclarations.length === 0 &&
        JSON.stringify(packageSkills) === JSON.stringify(expected) &&
        JSON.stringify(written) === JSON.stringify(expected) &&
        shapedExport.danglingSkills.length === 0 &&
        alphaExported.length === alphaOnDisk.length &&
        alphaExported.every((byte, index) => byte === alphaOnDisk[index]) &&
        // 包级技能带的引用文件也要一起落盘，与 `skill/file` 逐字节相同
        stepsExported.length === stepsWire.length &&
        stepsExported.every((byte, index) => byte === stepsWire[index]),
      {
        packageSkills,
        memberDeclarations,
        written,
        dangling: shapedExport.danglingSkills.map((entry) => entry.id),
        alphaBytes: alphaExported.length,
        stepsBytes: stepsExported.length,
      },
    );
  } else {
    check("EX-011.package-level-team-skills", false, "package-skills-team produced no team");
  }
} catch (error) {
  console.error("ERROR:", String(error).slice(0, 1200));
  failures += 1;
} finally {
  if (workdir) {
    await rm(workdir, { recursive: true, force: true }).catch(() => undefined);
    console.log(`CLEANED ${workdir}`);
  }
  const verdict = failures === 0 ? "PASS" : `FAIL(${failures})`;
  console.log(`\nRESULT ${verdict} data=${server.dataDir}`);
  await server.close();
  process.exit(failures === 0 ? 0 : 1);
}
