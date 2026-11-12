#!/usr/bin/env bun
/**
 * Seed the dev-channel database with learning data that triggers the review
 * (复习) and quiz (测验) features immediately, for pre-launch verification.
 *
 * Why: review cards are first due at the NEXT review day's 02:00 local
 * (first_review_due_at), and a quiz unlocks only after 6 settled nodes
 * (QUIZ_TRIGGER_SETTLEMENTS) — both need real time / real accumulation on a
 * clean dev database. This script fabricates the accumulated state directly.
 *
 * Two modes (v3, u6):
 *
 *   traditional (default) — unchanged from v2:
 *   - one synthetic course `launch-sim 触发数据` (8 lessons × 1 objective
 *     question each, configs valid under the real `evaluate()` grader);
 *   - 8 `node-completed` XP events spread over the past 4–7 local days
 *     (54 XP/day) → quiz due (8/6) + XP calendar history + checkin history;
 *   - 8 review cards: 4 bare cards overdue 1/2/4/7 days + 4 cards with push
 *     history (review_log rows, rating_source 'self') so ranking,
 *     retrievability buckets, weak-first quiz signals and the memory health
 *     panel all have signal;
 *   - 4 checkin rows for those past days (today is left unlocked on purpose —
 *     real review/quiz settlements should lock it during verification).
 *
 *   graph (`--graph`) — learning-graph course variant (u6, 零 LLM):
 *   - one synthetic course `launch-sim 图课程触发数据` (course_kind=
 *     'learning_graph' + learning_goal + full structure_json envelope, shape
 *     mirrored from learning_smoke.rs `valid_structure_seed_envelope`);
 *   - the minimal 5-table graph set: courses → 1 module → 8 node lessons
 *     (est/content_generated=1, activities reuse the v2 config shapes) →
 *     learning_concept_registry → learning_lesson_concepts (teaches ×7 +
 *     assumes ×1, tiers know/apply — the ready set reads as true at query
 *     time, no prerequisite edges stored);
 *   - the first `--settled <n>` nodes completed (default 6; lesson_progress
 *     rows + `node-completed` XP events over days 4–6, 54 XP/day) → quiz
 *     settled n/6: n = 6 keeps the graph workspace entry DUE, n < 6 keeps it
 *     NOT due (u7 acceptance: one due + one not-due course on the same DB);
 *     remaining nodes stay 未开始; review cards are seeded as usual regardless
 *     of `--settled` (照造不裁);
 *   - soft-gate acceptance arm: the supplier node (节点 6) holds one LOW-R
 *     card (stability 1.0d, last push 3 days ago → predicted recall ≈ 0.77
 *     < R_GATE 0.85) so the workspace's `graph.blocked` advice fires for
 *     节点 7 (default 6-settled plan); nodes 1–5 carry bare overdue cards (as
 *     a real completion chain would have seeded them — stability 0 reads as
 *     full recall, no gate effect);
 *   - one applied growth batch row so 学习记录 has a timeline entry.
 *
 * Deliberately NOT seeded: `quiz-completed` events (keeps quiz due), custom
 * questions, today's checkin.
 *
 * Known divergence (by design): fabricated settlements bypass the companion
 * XP bridge, so the companion balance does not see this history — the
 * learning ledger surfaces (calendar / checkin / triggers) are unaffected.
 * The checkin cleanup fingerprint (goal 40 + reviewed_count 27/54) is shared
 * by both modes, so `--fresh` on one mode also clears same-fingerprint
 * checkins left by the other — verification DBs rarely hold both at once.
 *
 * SAFETY: close ALL app instances (desktop, `bun run dev`, `dev:web`,
 * `nomicore`) first — the backup step (`VACUUM INTO`) doubles as the lock
 * probe and fails fast on a live database. Dev channel only: the default
 * target is the `Nomi-dev` data dir; pass `--db` to aim elsewhere.
 *
 * Usage:
 *   bun scripts/seed-learning-triggers.mjs [--graph] [--settled <n>] [--db <path>] [--fresh] [--dry-run]
 *     --db       target SQLite path (default: dev channel data dir)
 *     --graph    seed the learning-graph course variant instead of the
 *                traditional one (independent marker course)
 *     --settled  graph mode only: completed node count, integer 1–6
 *                (default 6 = quiz due); below 6 = quiz not due
 *     --fresh    delete the previous seed run's derived rows, reseed
 *     --dry-run  print the plan without writing anything
 *
 * Idempotent: reruns detect the marker course and skip; `--fresh` rebuilds.
 *
 * Built-in assertions (u6 修复 F8): after seeding, the script asserts the core
 * board in-script (row counts / supplier low-R card / due flip / marker
 * idempotency) and exits non-zero with an explicit failure line on any
 * mismatch — rerun to verify, no manual readout diffing.
 */
import { Database } from 'bun:sqlite';
import { existsSync, statSync } from 'node:fs';
import { homedir, platform } from 'node:os';
import { join } from 'node:path';

const DAY_MS = 86_400_000;
const HOUR_MS = 3_600_000;
const MIN_MS = 60_000;
/** Must stay in sync with nomifun-learning: QUIZ_TRIGGER_SETTLEMENTS. */
const QUIZ_TRIGGER_SETTLEMENTS = 6;
/** Must stay in sync with nomifun-learning: FSRS_ALGO. */
const FSRS_ALGO = 'fsrs6';
/** Must stay in sync with settle.rs: XP_FULL_BONUS + settle_node(est=25, full). */
const NODE_EST_MINUTES = 25;
const XP_FULL_BONUS = 2;
/** settle.rs: REVIEW_MIN_MINUTES — daily checkin goal default (j05 XP 口径). */
const DAILY_XP_GOAL_DEFAULT = 40;
/** Days between two fabricated pushes of one history card. */
const PUSH_GAP_DAYS = 4;
/** Marker course title — idempotency + --fresh cleanup anchor (traditional). */
const MARKER_TITLE = 'launch-sim 触发数据';
/** Marker course title for the graph variant (independent anchor, u6). */
const MARKER_TITLE_GRAPH = 'launch-sim 图课程触发数据';
/** Graph mode default: nodes completed up front (→ quiz settled 6/6, due).
 * `--settled <n>` overrides the count downward (1–6, not-due arm for u7). */
const GRAPH_COMPLETED_NODES = 6;
/**
 * Graph mode soft-gate arm (ADR-0009 Amendment 1): the supplier card's
 * stability days / last-push age. R = FSRS current_retrievability(1.0d,
 * ~3d) ≈ 0.77 < R_GATE (0.85) → the candidate assuming this supplier's
 * concept is blocked. Keep the values low/short — the whole point is a LOW
 * recall prediction.
 */
const GRAPH_SUPPLIER_STABILITY_DAYS = 1.0;
const GRAPH_SUPPLIER_LAST_PUSH_DAYS_AGO = 3;

// ---------------------------------------------------------------------------
// argv
// ---------------------------------------------------------------------------

function parseArgs(argv) {
  const args = { db: null, fresh: false, dryRun: false, graph: false, settled: null };
  for (let i = 0; i < argv.length; i += 1) {
    const value = argv[i];
    if (value === '--db') {
      i += 1;
      args.db = argv[i];
      if (!args.db) throw new Error('--db requires a path');
    } else if (value === '--fresh') {
      args.fresh = true;
    } else if (value === '--dry-run') {
      args.dryRun = true;
    } else if (value === '--graph') {
      args.graph = true;
    } else if (value === '--settled') {
      i += 1;
      const n = Number(argv[i]);
      if (!Number.isInteger(n) || n < 1 || n > GRAPH_COMPLETED_NODES) {
        throw new Error(`--settled requires an integer 1-${GRAPH_COMPLETED_NODES}`);
      }
      args.settled = n;
    } else {
      throw new Error(`unknown argument: ${value}`);
    }
  }
  if (args.settled !== null && !args.graph) {
    throw new Error('--settled only applies to --graph mode');
  }
  return args;
}

// ---------------------------------------------------------------------------
// paths — mirror nomifun_app::cli::default_data_dir + the dev channel suffix
// ---------------------------------------------------------------------------

function appDataBase() {
  const home = homedir();
  switch (platform()) {
    case 'darwin':
      return join(home, 'Library', 'Application Support');
    case 'win32':
      return process.env.LOCALAPPDATA ?? join(home, 'AppData', 'Local');
    default:
      return process.env.XDG_DATA_HOME ?? join(home, '.local', 'share');
  }
}

function defaultDevDb() {
  return join(appDataBase(), 'Flowy', 'Nomi-dev', 'flowy-backend.db');
}

// ---------------------------------------------------------------------------
// ids — UUIDv7 (the DB CHECKs demand lowercase, version 7, variant [89ab])
// ---------------------------------------------------------------------------

function uuidv7() {
  const ts = Date.now();
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  // 48-bit big-endian millisecond timestamp.
  let rest = ts;
  for (let i = 5; i >= 0; i -= 1) {
    bytes[i] = Number(rest & 0xff);
    rest /= 256;
  }
  bytes[6] = 0x70 | (bytes[6] & 0x0f); // version 7
  bytes[8] = 0x80 | (bytes[8] & 0x3f); // variant [89ab]
  const hex = [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

// ---------------------------------------------------------------------------
// review-day math — mirror scheduler.rs (02:00 local rollover)
// ---------------------------------------------------------------------------

function reviewDayStartUtc(now, tzMinutes) {
  const offsetMs = tzMinutes * MIN_MS;
  const local = now + offsetMs;
  const localDay = Math.floor(local / DAY_MS);
  const dayStart = localDay * DAY_MS + 2 * HOUR_MS - offsetMs;
  return now < dayStart ? dayStart - DAY_MS : dayStart;
}

function reviewDayNumber(now, tzMinutes) {
  const localMs = reviewDayStartUtc(now, tzMinutes) + tzMinutes * MIN_MS;
  const date = new Date(localMs);
  return (
    date.getUTCFullYear() * 10_000 + (date.getUTCMonth() + 1) * 100 + date.getUTCDate()
  );
}

/** Wall-clock timestamp `daysAgo` days before now, at local hh:mm (mid-day by
 * construction — safely inside the review day regardless of the 02:00 roll). */
function localMoment(daysAgo, hours, minutes, now = Date.now()) {
  const midnight = new Date(now);
  midnight.setHours(0, 0, 0, 0);
  return midnight.getTime() - daysAgo * DAY_MS + hours * HOUR_MS + minutes * MIN_MS;
}

// ---------------------------------------------------------------------------
// seed content — every config is valid under progress.rs `evaluate()`
// ---------------------------------------------------------------------------

const LESSONS = [
  {
    title: '节点 1：光速',
    summary: '光在真空中的传播速度约为每秒 30 万公里，是宇宙中的速度上限。',
    kind: 'single_choice',
    prompt: '光在真空中的速度大约是多少？',
    config: { options: ['30 万公里/秒', '3 万公里/秒', '300 万公里/秒'], answer: '30 万公里/秒', explanation: '光速约 299,792 公里/秒，常取 30 万公里/秒。' },
  },
  {
    title: '节点 2：素数',
    summary: '素数只能被 1 和自身整除；2 是唯一的偶素数。',
    kind: 'true_false',
    prompt: '判断：所有素数都是奇数。',
    config: { options: [], answer: false, explanation: '2 是素数且是偶数，所以"所有素数都是奇数"为假。' },
  },
  {
    title: '节点 3：三角形内角和',
    summary: '平面几何中任意三角形的三个内角之和恒为 180 度。',
    kind: 'fill_in_blank',
    prompt: '一个三角形的内角和是____度。',
    config: { options: [], answer: ['180', '180°'], explanation: '三角形内角和恒为 180 度。' },
  },
  {
    title: '节点 4：无理数',
    summary: '无理数是无限不循环小数，不能写成两个整数之比。',
    kind: 'multi_choice',
    prompt: '下列哪些数是无理数？（多选）',
    config: { options: ['π', '√2', '0.5', 'e'], answer: ['π', '√2', 'e'], explanation: 'π、√2、e 都是无理数；0.5 是有理数。' },
  },
  {
    title: '节点 5：乘法',
    summary: '两位数乘法的进位规则：先算个位，再算十位，逐位进位。',
    kind: 'numeric',
    prompt: '计算：12 × 8 = ?',
    config: { options: [], answer: 96, explanation: '12 × 8 = 96。' },
  },
  {
    title: '节点 6：长度单位',
    summary: '常用长度单位从大到小：米、分米、厘米、毫米，相邻进率 10。',
    kind: 'ordering',
    prompt: '把下列长度单位按从小到大排序。',
    config: { options: ['分米', '毫米', '米', '厘米'], answer: ['毫米', '厘米', '分米', '米'], explanation: '毫米 < 厘米 < 分米 < 米，相邻进率 10。' },
  },
  {
    title: '节点 7：元素符号',
    summary: '常见元素的化学符号：氢 H、氧 O、碳 C。',
    kind: 'matching',
    prompt: '把元素与对应的化学符号配对。',
    config: { options: ['氢', '氧', '碳'], matches: ['H', 'O', 'C', 'Fe'], answer: ['H', 'O', 'C'], explanation: '氢 H、氧 O、碳 C；Fe 是铁的符号，是干扰项。' },
  },
  {
    title: '节点 8：水的化学式',
    summary: '水由两个氢原子和一个氧原子组成。',
    kind: 'single_choice',
    prompt: '水的化学式是？',
    config: { options: ['H₂O', 'CO₂', 'O₂'], answer: 'H₂O', explanation: '水分子由 2 个氢原子和 1 个氧原子构成，写作 H₂O。' },
  },
];

/** Review-card schedule shape: group A (bare cards) then group B (history). */
const CARD_PLANS = [
  // bare cards: overdue 1/2/4/7 days, never pushed
  { daysOverdue: 1, pushes: null },
  { daysOverdue: 2, pushes: null },
  { daysOverdue: 4, pushes: null },
  { daysOverdue: 7, pushes: null },
  // history cards: [reviewCount, stability, difficulty, lastDaysAgo, dueDaysAgo, lapses]
  { daysOverdue: 0.5, pushes: { count: 2, stability: 3.5, difficulty: 4.5, lastDaysAgo: 1.5, lapses: 0 } },
  { daysOverdue: 1, pushes: { count: 3, stability: 8.0, difficulty: 6.0, lastDaysAgo: 2.5, lapses: 1 } },
  { daysOverdue: 1.5, pushes: { count: 4, stability: 14.0, difficulty: 7.0, lastDaysAgo: 3.5, lapses: 0 } },
  { daysOverdue: 2, pushes: { count: 5, stability: 26.0, difficulty: 8.2, lastDaysAgo: 5.0, lapses: 2 } },
];

/** Graph mode card plans (u6 软闸臂): fixed six — nodes 1–5 carry bare
 * overdue cards, the supplier node (节点 6, index 5) carries the ONE low-R
 * card (GRAPH_SUPPLIER_*). Deliberately NOT trimmed by `--settled` (照造不裁)
 * — the review surface keeps its signal in every graph arm. */
const GRAPH_CARD_PLANS = [
  { daysOverdue: 1, pushes: null },
  { daysOverdue: 2, pushes: null },
  { daysOverdue: 4, pushes: null },
  { daysOverdue: 7, pushes: null },
  { daysOverdue: 0.5, pushes: null },
  {
    daysOverdue: 1,
    pushes: {
      count: 2,
      stability: GRAPH_SUPPLIER_STABILITY_DAYS,
      difficulty: 6.5,
      lastDaysAgo: GRAPH_SUPPLIER_LAST_PUSH_DAYS_AGO,
      lapses: 1,
    },
  },
];

// ---------------------------------------------------------------------------
// graph mode (v3, u6): 8-node learning-graph course. Nodes 1–6 completed
// (quiz due 6/6); 节点 7 assumes 节点 6's concept → soft-gate candidate;
// 节点 8 assumes nothing → the one ready/recommended node. Concept net:
// teaches ×7 + assumes ×1 (tiers know/apply), so the concept table and the
// ready-set read both roles and both tiers.
// ---------------------------------------------------------------------------

const GRAPH_LESSONS = [
  {
    title: '节点 1：光速',
    summary: '光在真空中的传播速度约为每秒 30 万公里，是宇宙中的速度上限。',
    spine: '物质变化', stage: 'basic',
    kind: 'single_choice',
    prompt: '光在真空中的速度大约是多少？',
    config: { options: ['30 万公里/秒', '3 万公里/秒', '300 万公里/秒'], answer: '30 万公里/秒', explanation: '光速约 299,792 公里/秒，常取 30 万公里/秒。' },
    concept: { canonical: '光速', definition: '光在真空中的传播速度，约每秒 30 万公里，是物理学的基准常数之一。', tier: 'know' },
  },
  {
    title: '节点 2：素数',
    summary: '素数只能被 1 和自身整除；2 是唯一的偶素数。',
    spine: '数与形', stage: 'basic',
    kind: 'true_false',
    prompt: '判断：所有素数都是奇数。',
    config: { options: [], answer: false, explanation: '2 是素数且是偶数，所以"所有素数都是奇数"为假。' },
    concept: { canonical: '素数', definition: '大于 1 且只能被 1 和自身整除的自然数；2 是唯一的偶素数。', tier: 'know' },
  },
  {
    title: '节点 3：三角形内角和',
    summary: '平面几何中任意三角形的三个内角之和恒为 180 度。',
    spine: '数与形', stage: 'basic',
    kind: 'fill_in_blank',
    prompt: '一个三角形的内角和是____度。',
    config: { options: [], answer: ['180', '180°'], explanation: '三角形内角和恒为 180 度。' },
    concept: { canonical: '三角形内角和', definition: '平面几何中任意三角形三个内角之和恒为 180 度。', tier: 'know' },
  },
  {
    title: '节点 4：无理数',
    summary: '无理数是无限不循环小数，不能写成两个整数之比。',
    spine: '数与形', stage: 'advanced',
    kind: 'multi_choice',
    prompt: '下列哪些数是无理数？（多选）',
    config: { options: ['π', '√2', '0.5', 'e'], answer: ['π', '√2', 'e'], explanation: 'π、√2、e 都是无理数；0.5 是有理数。' },
    concept: { canonical: '无理数', definition: '无限不循环小数，不能写成两个整数之比；与有理数一并构成实数。', tier: 'apply' },
  },
  {
    title: '节点 5：乘法',
    summary: '两位数乘法的进位规则：先算个位，再算十位，逐位进位。',
    spine: '数与形', stage: 'basic',
    kind: 'numeric',
    prompt: '计算：12 × 8 = ?',
    config: { options: [], answer: 96, explanation: '12 × 8 = 96。' },
    concept: { canonical: '乘法进位', definition: '多位数乘法中逐位计算并向高位进位的规则。', tier: 'know' },
  },
  {
    title: '节点 6：水的化学式',
    summary: '水由两个氢原子和一个氧原子组成，化学式 H₂O。',
    spine: '物质变化', stage: 'advanced',
    kind: 'single_choice',
    prompt: '水的化学式是？',
    config: { options: ['H₂O', 'CO₂', 'O₂'], answer: 'H₂O', explanation: '水分子由 2 个氢原子和 1 个氧原子构成，写作 H₂O。' },
    concept: { canonical: '水的组成', definition: '水分子由两个氢原子和一个氧原子构成，化学式写作 H₂O。', tier: 'apply' },
  },
  {
    // 软闸候选（graph.blocked）：assumes 节点 6 教出的「水的组成」，
    // 而节点 6 名下有一张低 R 卡 → 供给节点代表回忆率 < 0.85。
    title: '节点 7：电解水',
    summary: '通电分解水：正极氧气、负极氢气，体积比约 1:2。',
    spine: '物质变化', stage: 'advanced',
    kind: 'true_false',
    prompt: '判断：电解水会生成氢气和氧气。',
    config: { options: [], answer: true, explanation: '电解水 2H₂O → 2H₂↑ + O₂↑，负极氢气、正极氧气。' },
    concept: null,
  },
  {
    // 就绪节点（recommended[0]）：零 assumes，发布即就绪。
    title: '节点 8：化学方程式配平',
    summary: '配平的依据是质量守恒定律：反应前后各元素原子数目不变。',
    spine: '物质变化', stage: 'advanced',
    kind: 'single_choice',
    prompt: '化学方程式配平的直接依据是？',
    config: { options: ['质量守恒定律', '能量守恒定律', '电荷守恒定律'], answer: '质量守恒定律', explanation: '配平保证反应前后各元素原子数目相等，依据是质量守恒定律。' },
    concept: { canonical: '质量守恒', definition: '化学反应前后各元素原子数目与总质量保持不变的定律。', tier: 'know' },
  },
];

/** assumes 挂接（先修语义读时计算）：候选节点 index → 供给概念 canonical
 * 字符串（u6 修复：概念按 canonical 寻址，structure edges 与 lesson_concepts
 * 两处共用同一寻址，免 index 中转的同体换算函数）。 */
const GRAPH_ASSUMES = [{ lesson: 6, concept: '水的组成', tier: 'know' }];

/** 全量合法 structure_json 信封（形态抄 learning_smoke.rs
 * valid_structure_seed_envelope：八标量键齐 + domain + spines + blocks +
 * edges + gaps + quality + terrain.doors，缩盘到 2 主干 × 8 块）。 */
function graphStructureEnvelope(goal, now) {
  const spines = [
    { name: '数与形', definition: '数与形的初等结构：素数、几何与运算。', key_points: ['素数判定', '内角和恒等式', '实数分类'], instance: '能独立完成素数判定与实数分类，并复述内角和恒等式' },
    { name: '物质变化', definition: '物质组成与化学变化的初等结构。', key_points: ['水的组成', '电解产物', '配平依据'], instance: '能独立写出水的化学式并解释电解与配平依据' },
  ];
  const spineOf = (lesson) => spines.find((spine) => spine.name === lesson.spine);
  const blocks = GRAPH_LESSONS.map((lesson, index) => ({
    name: lesson.title,
    slug: `block-0${index + 1}`,
    spine: lesson.spine,
    stage: lesson.stage,
    note: lesson.summary,
  }));
  return {
    version: 1,
    generated_at: now,
    structure: {
      goal,
      goal_type: 'capability',
      proficiency_statement: '达成后能独立复述数与形初等结论，并完成水的组成、电解与配平判据的书面表述',
      domain_type: 'theory',
      form_reference: 'Flowy 学习结构模板 v1',
      source_tier: 'catalog',
      generated_by: 'seed-learning-triggers',
      disclaimer: '种子为静态合法盘面，仅供真机走查',
      domain: {
        core_definition: '数与形初等结论 + 物质组成与变化判据的最小合法结构',
        includes: ['素数与实数', '三角形内角和', '水的组成与电解'],
        excludes: ['真实课程内容', '学习者数据', '生产库'],
        prerequisite_doors: ['G1'],
        audience: '走查操作者',
        scenarios: ['就绪集走查', '软闸走查'],
        granularity: '每块一个可独立教学的主题',
      },
      spines: spines.map((spine) => ({
        name: spine.name,
        definition: spine.definition,
        key_points: spine.key_points,
        instance: spine.instance,
      })),
      blocks,
      edges: GRAPH_ASSUMES.map((link) => ({
        source: GRAPH_LESSONS[link.lesson].title,
        target: GRAPH_LESSONS.find((lesson) => lesson.concept?.canonical === link.concept).title,
        type: 'assumes',
        tier: link.tier,
        note: '先认化学式再谈电解',
      })),
      gaps: [
        { item: '应用情境覆盖不足', direction: '补情境块' },
        { item: '实数运算未展开', direction: '补运算判读' },
      ],
      quality: [
        { check: '主干数在 6–12 档', status: 'met' },
        { check: '块名一物一名', status: 'met' },
        { check: '边端点全部可解析', status: 'met' },
        { check: '无顺序承诺', status: 'met' },
        { check: '盲区已标注', status: 'met' },
      ],
    },
    terrain: {
      goal,
      learner_basis_assumed: '已具备四则运算与常见物质名称的基线认识',
      generated_by: 'seed-learning-triggers',
      disclaimer: '种子地形，仅供门口对账',
      doors: [{ id: 'G1', criterion: '能读懂结构快照的块与边', kind: 'hard' }],
    },
  };
}

/** Push-history plausibility: per push index (0-based), stability before the
 * push and a rating (1=again .. 4=easy). Lapses inject an `again` push. */
function pushSequence(pushes) {
  const { count, lapses } = pushes;
  const lapsePositions = new Set();
  if (lapses > 0) {
    // distribute lapse pushes between the later pushes (never the first)
    for (let k = 0; k < lapses; k += 1) {
      lapsePositions.add(Math.max(1, Math.floor(((k + 1) * count) / (lapses + 1))));
    }
  }
  const sequence = [];
  let stability = 2.0;
  for (let k = 0; k < count; k += 1) {
    const lapsed = lapsePositions.has(k);
    // stability path: again → collapse toward 2; good → grow ~×1.6
    const stabilityAfter = lapsed ? 1.8 : Math.max(3.5, stability * 1.6);
    sequence.push({
      rating: lapsed ? 1 : 3,
      // the first push has no memory state yet (review_count = 0 snapshot)
      stabilityBefore: k === 0 ? null : stability,
    });
    stability = stabilityAfter;
  }
  return sequence;
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

function fail(message) {
  console.error(`seed-learning-triggers: ${message}`);
  process.exit(1);
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const isGraph = args.graph;
  const markerTitle = isGraph ? MARKER_TITLE_GRAPH : MARKER_TITLE;
  const lessons = isGraph ? GRAPH_LESSONS : LESSONS;
  const dbPath = args.db ?? defaultDevDb();
  if (!existsSync(dbPath)) {
    fail(`database not found: ${dbPath} (pass --db to aim elsewhere)`);
  }
  const stat = statSync(dbPath);
  if (!stat.isFile()) fail(`target is not a file: ${dbPath}`);

  console.log(`mode: ${isGraph ? 'graph (--graph, learning_graph course)' : 'traditional'}`);
  console.log(`database: ${dbPath}`);
  const db = new Database(dbPath);
  db.exec('PRAGMA busy_timeout = 1500');

  const now = Date.now();

  // tz mirror of checkin.rs:tz_offset_minutes (preference → machine local).
  const tzPref = db.query("SELECT value FROM client_preferences WHERE key = 'learning.tzOffsetMinutes'").get();
  let tz = -new Date().getTimezoneOffset();
  if (tzPref) {
    try {
      const parsed = JSON.parse(tzPref.value);
      if (Number.isInteger(parsed) && parsed >= -1440 && parsed <= 1440) tz = parsed;
    } catch {
      // malformed preference → machine local (same fallback as the backend)
    }
  }
  console.log(`tz offset: ${tz} min east of UTC`);
  console.log(`now: ${new Date(now).toISOString()}`);

  // owner = installation owner (the user the dev backend authenticates as)
  const owner = db
    .query(
      "SELECT u.user_id, u.username FROM installation_identity i \
       JOIN users u ON u.user_id = i.owner_user_id WHERE i.singleton_key = 'installation'",
    )
    .get();
  if (!owner) fail('installation owner not found (installation_identity empty) — run the app once first');
  console.log(`owner: ${owner.username ?? '(unnamed)'} ${owner.user_id}`);

  const marker = db.query('SELECT course_id FROM learning_courses WHERE title = ?').get(markerTitle);
  if (marker && !args.fresh) {
    console.log(`marker course exists (${marker.course_id}); nothing to do (pass --fresh to rebuild)`);
    db.close();
    return;
  }

  const completedCount = isGraph ? (args.settled ?? GRAPH_COMPLETED_NODES) : lessons.length;
  if (isGraph && !args.settled && completedCount < QUIZ_TRIGGER_SETTLEMENTS) {
    // wiring guard (default plan only — an explicit `--settled` below the
    // trigger IS the deliberate not-due arm, u7): keep the node plan and the
    // trigger constant in sync.
    fail(`graph plan has ${completedCount} completed nodes but quiz needs ${QUIZ_TRIGGER_SETTLEMENTS}`);
  }

  if (args.dryRun) {
    console.log('\n[dry-run] plan:');
    console.log(`  course        1   (${markerTitle}${isGraph ? ', course_kind=learning_graph + structure_json envelope' : ''})`);
    console.log('  module        1');
    console.log(`  lessons       ${lessons.length} (est ${NODE_EST_MINUTES}, content_generated=1)`);
    console.log(`  activities    ${lessons.length} (${[...new Set(lessons.map((l) => l.kind))].join(', ')})`);
    if (isGraph) {
      console.log(`  concepts      ${GRAPH_LESSONS.filter((l) => l.concept).length} registry rows (teaches ${GRAPH_LESSONS.filter((l) => l.concept).length} / assumes ${GRAPH_ASSUMES.length})`);
      console.log('  growth batch  1 (seq 1, applied)');
      console.log(`  completed     ${completedCount} nodes → quiz settled ${completedCount}/${QUIZ_TRIGGER_SETTLEMENTS} (${completedCount >= QUIZ_TRIGGER_SETTLEMENTS ? 'due' : 'NOT due'})`);
      console.log(`  open nodes    ${lessons.length - completedCount} (节点 8 ready${completedCount >= QUIZ_TRIGGER_SETTLEMENTS ? ' · 节点 7 gate-blocked candidate' : ''})`);
      console.log(`  review items  ${GRAPH_CARD_PLANS.length} (5 bare overdue + 1 low-R supplier card + review_log${completedCount >= QUIZ_TRIGGER_SETTLEMENTS ? ' → graph.blocked' : ''})`);
    } else {
      console.log('  enrollment    1');
      console.log(`  xp events     ${lessons.length} node-completed, days 4-7 ago, 54 XP/day`);
      console.log(`  review items  ${CARD_PLANS.length} (4 bare overdue + 4 with push history + review_log)`);
    }
    console.log('  checkins      past days (today left unlocked)');
    db.close();
    return;
  }

  // ---- backup (also the lock probe: a running backend fails here) ----------
  // Second-resolution stamp, de-collided for reruns within the same second
  // (a failed attempt then immediate retry must not refuse on its own
  // earlier backup file).
  const stamp = new Date(now).toISOString().replace(/[-:T]/g, '').slice(0, 14);
  let backupPath = `${dbPath}.pre-seed-learning-${stamp}`;
  for (let attempt = 1; existsSync(backupPath); attempt += 1) {
    backupPath = `${dbPath}.pre-seed-learning-${stamp}-${attempt}`;
  }
  try {
    db.exec(`VACUUM INTO '${backupPath.replace(/'/g, "''")}'`);
  } catch (error) {
    fail(
      `backup failed: ${error.message}\n` +
        '  the database is probably locked by a running backend — close ALL app instances and retry',
    );
  }
  console.log(`backup: ${backupPath}`);

  // ---- fresh cleanup -------------------------------------------------------
  db.exec('BEGIN');
  try {
    if (marker && args.fresh) {
      cleanupFresh(db, owner.user_id, marker.course_id);
      console.log('fresh: previous launch-sim data removed');
    }

    // ---- course / module / lessons / activities / enrollment --------------
    const courseId = uuidv7();
    const moduleId = uuidv7();
    const enrollmentId = uuidv7();
    if (isGraph) {
      const goal = '图课程触发验证：走完就绪集 → 结算 → 检测到期 → 软闸条全链';
      db.run(
        'INSERT INTO learning_courses (course_id, title, description, domain, version, course_kind, learning_goal, learning_scope, structure_json, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 1, \'learning_graph\', ?, \'\', ?, ?, ?)',
        courseId,
        markerTitle,
        '由 scripts/seed-learning-triggers.mjs --graph 造数生成，用于图工作区检测入口/软闸/造数走查；验证完可整课删除。',
        'general',
        goal,
        JSON.stringify(graphStructureEnvelope(goal, now)),
        now,
        now,
      );
    } else {
      db.run(
        'INSERT INTO learning_courses (course_id, title, description, domain, version, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 1, ?, ?)',
        courseId,
        markerTitle,
        '由 scripts/seed-learning-triggers.mjs 造数生成，用于上线前复习/测验触发验证；验证完可整课删除。',
        'general',
        now,
        now,
      );
    }
    db.run(
      'INSERT INTO learning_modules (module_id, course_id, title, description, position) \
       VALUES (?, ?, ?, ?, 0)',
      moduleId,
      courseId,
      isGraph ? '学习图' : '模拟模块',
      '',
    );
    const lessonIds = [];
    const activityIds = [];
    lessons.forEach((lesson, index) => {
      const lessonId = uuidv7();
      const activityId = uuidv7();
      db.run(
        'INSERT INTO learning_lessons (lesson_id, module_id, title, summary, position, estimated_minutes, content_generated) \
         VALUES (?, ?, ?, ?, ?, ?, 1)',
        lessonId,
        moduleId,
        lesson.title,
        lesson.summary,
        index,
        NODE_EST_MINUTES,
      );
      db.run(
        "INSERT INTO learning_activities (activity_id, lesson_id, kind, prompt, config_json, position) \
         VALUES (?, ?, ?, ?, ?, 0)",
        activityId,
        lessonId,
        lesson.kind,
        lesson.prompt,
        JSON.stringify({ ...lesson.config, distractors: [], matches: lesson.config.matches ?? [] }),
      );
      lessonIds.push(lessonId);
      activityIds.push(activityId);
    });
    db.run(
      'INSERT INTO learning_enrollments (enrollment_id, user_id, course_id, enrolled_at, updated_at) \
       VALUES (?, ?, ?, ?, ?)',
      enrollmentId,
      owner.user_id,
      courseId,
      now,
      now,
    );

    // ---- concept net (graph mode, u6): registry + lesson hooks -----------
    if (isGraph) {
      const conceptIds = [];
      GRAPH_LESSONS.forEach((lesson) => {
        if (!lesson.concept) return;
        const conceptId = uuidv7();
        db.run(
          'INSERT INTO learning_concept_registry (concept_id, canonical, aliases_json, definition, created_at, updated_at) \
           VALUES (?, ?, \'[]\', ?, ?, ?)',
          conceptId,
          lesson.concept.canonical,
          lesson.concept.definition,
          now,
          now,
        );
        lesson.conceptId = conceptId;
        conceptIds.push(conceptId);
      });
      GRAPH_LESSONS.forEach((lesson, index) => {
        if (!lesson.concept) return;
        db.run(
          'INSERT INTO learning_lesson_concepts (lesson_id, concept_id, role, tier, ref_name) \
           VALUES (?, ?, \'teaches\', ?, NULL)',
          lessonIds[index],
          lesson.conceptId,
          lesson.concept.tier,
        );
      });
      GRAPH_ASSUMES.forEach((link) => {
        const supplier = GRAPH_LESSONS[link.lesson];
        const conceptId =
          GRAPH_LESSONS.find((candidate) => candidate.concept?.canonical === link.concept)?.conceptId ?? null;
        if (!conceptId) fail(`graph assumes link references unknown concept '${link.concept}'`);
        db.run(
          'INSERT INTO learning_lesson_concepts (lesson_id, concept_id, role, tier, ref_name) \
           VALUES (?, ?, \'assumes\', ?, NULL)',
          lessonIds[link.lesson],
          conceptId,
          link.tier,
        );
      });
      // 学习记录时间线种子：一条 applied 首批（生长史即课程史）。
      db.run(
        'INSERT INTO learning_growth_batches (batch_id, course_id, seq, node_ids_json, note, created_at, status) \
         VALUES (?, ?, 1, ?, ?, ?, \'applied\')',
        uuidv7(),
        courseId,
        JSON.stringify(lessonIds),
        '造数种子：首批生长（8 节点）',
        now,
      );
    }

    // ---- settlement history: node-completed, 2 per day, days 4..7 ago -----
    // Real-flow coherence (v2): a node-completed settlement implies the lesson
    // progress row reached 'completed' (progress.rs settles from completion),
    // so fabricate the matching learning_lesson_progress rows too — otherwise
    // the UI shows 未开始 lessons with a settled ledger (an unreachable state).
    // Graph mode (u6): only the first 6 nodes are completed — the ready-set
    // read (就绪集) needs open nodes, and 6 settlements exactly reach the
    // quiz trigger.
    const xpDays = []; // { ts, xp } per day for the checkin rows
    lessonIds.slice(0, completedCount).forEach((lessonId, index) => {
      const daysAgo = 4 + Math.floor(index / 2); // 2 nodes per day
      const ts = localMoment(daysAgo, 10, 5 + (index % 2) * 95 + index * 7);
      const earned = NODE_EST_MINUTES + XP_FULL_BONUS; // settle_node(full): est + bonus
      db.run(
        'INSERT INTO learning_lesson_progress \
         (enrollment_id, lesson_id, status, started_at, completed_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?)',
        enrollmentId,
        lessonId,
        'completed',
        ts - NODE_EST_MINUTES * MIN_MS,
        ts,
        now,
      );
      db.run(
        'INSERT INTO learning_xp_events \
         (event_id, user_id, ts, course_id, node_id, kind, earned_xp, xp_bonus, basis, ratio, origin, anchor) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)',
        uuidv7(),
        owner.user_id,
        ts,
        courseId,
        lessonId,
        'node-completed',
        earned,
        XP_FULL_BONUS,
        'full',
        'lesson-progress',
        `node:${lessonId}`,
      );
      const day = xpDays.find((d) => d.reviewDay === reviewDayNumber(ts, tz));
      if (day) day.xp += earned;
      else xpDays.push({ reviewDay: reviewDayNumber(ts, tz), completedAt: localMoment(daysAgo, 21, 30), xp: earned });
    });

    // ---- review cards + push-history logs ---------------------------------
    // Graph mode (u6 软闸臂): cards only on the completed nodes — a real
    // completion chain seeds one card per activity. Nodes 1–5 carry bare
    // overdue cards (stability 0 reads as full recall — no gate effect);
    // the supplier node (节点 6, index 5) carries ONE low-R card
    // (GRAPH_SUPPLIER_*): low stability + recent push → predicted recall
    // ≈ 0.77 < R_GATE 0.85 → ready_partition blocks its dependent (节点 7)
    // and the workspace renders the soft-gate advice bar.
    const cardPlans = isGraph ? GRAPH_CARD_PLANS : CARD_PLANS;
    let logCount = 0;
    cardPlans.forEach((plan, index) => {
      const reviewItemId = uuidv7();
      const dueAt = Math.round(now - plan.daysOverdue * DAY_MS);
      const pushes = plan.pushes;
      db.run(
        'INSERT INTO learning_review_items \
         (review_item_id, enrollment_id, activity_id, due_at, stability_days, difficulty, \
          review_count, lapse_count, last_reviewed_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)',
        reviewItemId,
        enrollmentId,
        activityIds[index],
        dueAt,
        pushes ? pushes.stability : 0,
        pushes ? pushes.difficulty : 5.0,
        pushes ? pushes.count : 0,
        pushes ? pushes.lapses : 0,
        pushes ? localMoment(pushes.lastDaysAgo, 19, 10 + index) : null,
        now,
      );
      if (!pushes) return;
      const sequence = pushSequence(pushes);
      sequence.forEach((push, k) => {
        // pushes are PUSH_GAP_DAYS apart, the last one at lastDaysAgo, evening local time
        const ts = localMoment(pushes.lastDaysAgo + (sequence.length - 1 - k) * PUSH_GAP_DAYS, 20, 15 + k);
        const isFirst = k === 0;
        db.run(
          'INSERT INTO learning_review_log \
           (log_id, user_id, source, item_id, rating, rating_source, elapsed_days, \
            stability_before, difficulty_before, r_pred, review_day, created_at, algo) \
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)',
          uuidv7(),
          owner.user_id,
          'course',
          reviewItemId,
          push.rating,
          'self',
          isFirst ? 0 : PUSH_GAP_DAYS,
          push.stabilityBefore,
          isFirst ? null : pushes.difficulty,
          isFirst ? null : 0.9 - k * 0.03,
          reviewDayNumber(ts, tz),
          ts,
          FSRS_ALGO,
        );
        logCount += 1;
      });
    });

    // ---- checkin history (today deliberately untouched) --------------------
    let checkinCount = 0;
    for (const day of xpDays) {
      const existing = db
        .query('SELECT 1 FROM learning_checkins WHERE user_id = ? AND review_day = ?')
        .get(owner.user_id, day.reviewDay);
      if (existing) continue; // a real checkin already owns this day
      db.run(
        'INSERT INTO learning_checkins (checkin_id, user_id, review_day, goal, reviewed_count, completed_at) \
         VALUES (?, ?, ?, ?, ?, ?)',
        uuidv7(),
        owner.user_id,
        day.reviewDay,
        DAILY_XP_GOAL_DEFAULT,
        day.xp,
        day.completedAt,
      );
      checkinCount += 1;
    }

    db.exec('COMMIT');
    console.log(`seeded: course ${courseId}, ${lessonIds.length} lessons, ${completedCount} xp events, ${cardPlans.length} cards, ${logCount} log rows, ${checkinCount} checkins`);
    printReadout(db, owner.user_id, courseId, now, tz, {
      isGraph,
      lessonIds,
      completedCount,
    });
    assertSeedInvariants(db, owner.user_id, courseId, {
      isGraph,
      markerTitle,
      lessonCount: lessons.length,
      completedCount,
      cardCount: cardPlans.length,
      now,
    });
  } catch (error) {
    db.exec('ROLLBACK');
    fail(error.message);
  }
  db.close();
}

/** --fresh: remove every row derived from the previous seed run. Checkins are
 * matched by the seed fingerprint (goal + reviewed_count) on the seeded days
 * so real checkins on the same days survive. v3 (u6): also clears the graph
 * artifacts — concept hooks, growth batches, and seed-created concepts that
 * no remaining lesson references (shared-pool concepts survive, mirroring the
 * learning-smoke 撤销 semantics). The deletions are no-ops for the
 * traditional marker course. */
function cleanupFresh(db, userId, courseId) {
  const lessonIds = db
    .query('SELECT lesson_id FROM learning_lessons l JOIN learning_modules m ON m.module_id = l.module_id WHERE m.course_id = ?')
    .all(courseId)
    .map((r) => r.lesson_id);
  const enrollmentIds = db
    .query('SELECT enrollment_id FROM learning_enrollments WHERE course_id = ?')
    .all(courseId)
    .map((r) => r.enrollment_id);
  const itemIds = enrollmentIds.length
    ? db
        .query(`SELECT review_item_id FROM learning_review_items WHERE enrollment_id IN (${enrollmentIds.map(() => '?').join(',')})`)
        .all(...enrollmentIds)
        .map((r) => r.review_item_id)
    : [];

  db.run('DELETE FROM learning_review_log WHERE user_id = ? AND item_id IN (' + (itemIds.map(() => '?').join(',') || 'NULL') + ')', userId, ...itemIds);
  if (itemIds.length) db.run(`DELETE FROM learning_review_items WHERE review_item_id IN (${itemIds.map(() => '?').join(',')})`, ...itemIds);
  db.run('DELETE FROM learning_xp_events WHERE course_id = ?', courseId);
  // checkins: only rows carrying the seed fingerprint (goal 40, reviewed_count
  // 54 = 2 nodes/day; --settled odd counts leave one half-day of 27)
  db.run(
    'DELETE FROM learning_checkins WHERE user_id = ? AND goal = ? AND reviewed_count IN (?, ?)',
    userId,
    DAILY_XP_GOAL_DEFAULT,
    NODE_EST_MINUTES + XP_FULL_BONUS,
    (NODE_EST_MINUTES + XP_FULL_BONUS) * 2,
  );
  if (enrollmentIds.length) {
    db.run(`DELETE FROM learning_lesson_progress WHERE enrollment_id IN (${enrollmentIds.map(() => '?').join(',')})`, ...enrollmentIds);
    db.run(`DELETE FROM learning_enrollments WHERE enrollment_id IN (${enrollmentIds.map(() => '?').join(',')})`, ...enrollmentIds);
  }
  // graph artifacts (u6): concept hooks reference lessons — collect the
  // seed-created concepts first, delete hooks, then sweep registry rows that
  // no remaining lesson_concepts row references.
  if (lessonIds.length) {
    const conceptIds = db
      .query(`SELECT DISTINCT concept_id FROM learning_lesson_concepts WHERE lesson_id IN (${lessonIds.map(() => '?').join(',')})`)
      .all(...lessonIds)
      .map((r) => r.concept_id);
    db.run(`DELETE FROM learning_lesson_concepts WHERE lesson_id IN (${lessonIds.map(() => '?').join(',')})`, ...lessonIds);
    if (conceptIds.length) {
      db.run(
        `DELETE FROM learning_concept_registry WHERE concept_id IN (${conceptIds.map(() => '?').join(',')}) \
         AND concept_id NOT IN (SELECT DISTINCT concept_id FROM learning_lesson_concepts)`,
        ...conceptIds,
      );
    }
    db.run(`DELETE FROM learning_activities WHERE lesson_id IN (${lessonIds.map(() => '?').join(',')})`, ...lessonIds);
    db.run(`DELETE FROM learning_lessons WHERE lesson_id IN (${lessonIds.map(() => '?').join(',')})`, ...lessonIds);
  }
  db.run('DELETE FROM learning_growth_batches WHERE course_id = ?', courseId);
  db.run('DELETE FROM learning_modules WHERE course_id = ?', courseId);
  db.run('DELETE FROM learning_courses WHERE course_id = ?', courseId);
}

/** App-口径 readout: the same SQL shapes the backend uses. */
function printReadout(db, userId, courseId, now, tz, seedInfo = { isGraph: false, lessonIds: [], completedCount: 0 }) {
  const dueCount = db
    .query(
      'SELECT COUNT(*) AS n FROM learning_review_items r \
       JOIN learning_enrollments e ON e.enrollment_id = r.enrollment_id \
       JOIN learning_courses c ON c.course_id = e.course_id AND c.deleted_at IS NULL AND c.enabled = 1 \
       WHERE e.user_id = ? AND r.due_at <= ? AND r.archived_at IS NULL AND r.edit_pending_at IS NULL',
    )
    .get(userId, now).n;
  const settledNodes = db
    .query(
      "SELECT COUNT(*) AS n FROM learning_xp_events e \
       WHERE e.user_id = ? AND e.course_id = ? AND e.kind = 'node-completed' AND e.node_id IS NOT NULL \
         AND NOT EXISTS (SELECT 1 FROM learning_xp_events q \
           WHERE q.user_id = e.user_id AND q.course_id = e.course_id \
             AND q.kind = 'quiz-completed' AND q.rowid > e.rowid)",
    )
    .get(userId, courseId).n;
  const todayXp = db
    .query('SELECT COALESCE(SUM(earned_xp), 0) AS n FROM learning_xp_events WHERE user_id = ? AND ts >= ?')
    .get(userId, reviewDayStartUtc(now, tz)).n;
  const checkinDays = db
    .query('SELECT COUNT(*) AS n FROM learning_checkins WHERE user_id = ?')
    .get(userId).n;

  console.log('\nreadout (app-口径):');
  console.log(`  due review cards : ${dueCount}  (review banner should show 今日复习)`);
  console.log(`  quiz settled     : ${settledNodes}/${QUIZ_TRIGGER_SETTLEMENTS}  ${settledNodes >= QUIZ_TRIGGER_SETTLEMENTS ? '→ quiz DUE' : '→ not due'}`);
  console.log(`  today XP         : ${todayXp}  (today's checkin stays unlocked until real pushes reach the goal)`);
  console.log(`  checkin history  : ${checkinDays} locked days`);

  if (seedInfo.isGraph) {
    const openNodes = seedInfo.lessonIds.length - seedInfo.completedCount;
    const hooks = db
      .query(
        'SELECT role, COUNT(*) AS n FROM learning_lesson_concepts WHERE lesson_id IN (' +
          (seedInfo.lessonIds.map(() => '?').join(',') || 'NULL') +
          ') GROUP BY role',
      )
      .all(...seedInfo.lessonIds);
    const teaches = hooks.find((row) => row.role === 'teaches')?.n ?? 0;
    const assumes = hooks.find((row) => row.role === 'assumes')?.n ?? 0;
    const lowRCards = db
      .query(
        'SELECT COUNT(*) AS n FROM learning_review_items ri \
         JOIN learning_activities a ON a.activity_id = ri.activity_id \
         JOIN learning_lessons l ON l.lesson_id = a.lesson_id \
         JOIN learning_modules m ON m.module_id = l.module_id \
         WHERE m.course_id = ? AND ri.stability_days > 0 AND ri.stability_days <= 2 \
           AND ri.last_reviewed_at IS NOT NULL AND ri.last_reviewed_at >= ?',
      )
      .get(courseId, now - 4 * DAY_MS).n;
    console.log('\ngraph readout (软闸臂, u6):');
    const gateArmed = seedInfo.completedCount >= QUIZ_TRIGGER_SETTLEMENTS;
    console.log(`  open nodes        : ${openNodes}  (节点 8 ready → recommended${gateArmed ? ' · 节点 7 gate-blocked candidate' : ''})`);
    console.log(`  concept hooks     : teaches ${teaches} / assumes ${assumes}  (concept table should show both roles)`);
    console.log(`  supplier low-R    : ${lowRCards} card(s)  (stability ≤ 2d + last push ≤ 4d ago → R < 0.85${gateArmed ? ' → graph.blocked expected' : ' → gate idle below trigger'})`);
  }

  console.log('\nnext steps:');
  if (seedInfo.isGraph) {
    console.log('  1. start the dev app and open the graph course (图课程触发数据)');
    console.log('  2. expect: ready set shows 节点 8 · soft-gate bar for 节点 7 · quiz entry DUE');
    console.log(`  3. quiz API: GET /api/learning/courses/${courseId}/quiz/progress`);
  } else {
    console.log('  1. start the dev app and verify the review surface');
    console.log(`  2. quiz API: GET /api/learning/courses/${courseId}/quiz/progress`);
  }
  console.log(`  next steps doc: docs/tracker/learning-launch-sim/tickets/s1-seed-learning-triggers.md`);
}

// ---------------------------------------------------------------------------
// built-in assertions (u6 修复 F8)：核心盘面断言进脚本，重跑即验——mismatch
// 非零退出 + 明确失败行打印（graph 与传统两模式都过断言段）。断言全部落在
// 种子自控的 course 域事实（行数 / 软闸低 R 卡 / due 翻转 / marker 幂等），
// 不做全局计数（打卡等跨臂共存下不稳定的事实由 readout 打印承载）。
// ---------------------------------------------------------------------------

function assertSeedInvariants(db, userId, courseId, ctx) {
  const failures = [];
  let checks = 0;
  const expect = (name, actual, expected) => {
    checks += 1;
    if (actual !== expected) {
      failures.push(`  ASSERT FAIL ${name}: expected ${expected}, got ${actual}`);
    }
  };
  const scalar = (sql, ...params) => db.query(sql).get(...params)?.n ?? 0;

  // marker 幂等：标记课程恰一行（重跑 skip / --fresh 先删后插，都不该双行）
  expect(
    'marker course rows',
    scalar('SELECT COUNT(*) AS n FROM learning_courses WHERE title = ?', ctx.markerTitle),
    1,
  );
  if (ctx.isGraph) {
    expect(
      'marker course_kind=learning_graph',
      scalar(
        "SELECT COUNT(*) AS n FROM learning_courses WHERE title = ? AND course_kind = 'learning_graph'",
        ctx.markerTitle,
      ),
      1,
    );
  }

  // 行数：课程域内 5 表（lesson/activities/progress 与结算事件）
  const lessonIds = db
    .query(
      'SELECT lesson_id FROM learning_lessons l JOIN learning_modules m ON m.module_id = l.module_id WHERE m.course_id = ?',
    )
    .all(courseId)
    .map((r) => r.lesson_id);
  const lessonIn = lessonIds.map(() => '?').join(',') || 'NULL';
  expect('lessons', lessonIds.length, ctx.lessonCount);
  expect(
    'activities',
    scalar(`SELECT COUNT(*) AS n FROM learning_activities WHERE lesson_id IN (${lessonIn})`, ...lessonIds),
    ctx.lessonCount,
  );
  const enrollment = db
    .query('SELECT enrollment_id FROM learning_enrollments WHERE user_id = ? AND course_id = ?')
    .get(userId, courseId);
  expect('enrollment', enrollment ? 1 : 0, 1);
  expect(
    'lesson_progress rows (only the settled nodes)',
    scalar('SELECT COUNT(*) AS n FROM learning_lesson_progress WHERE enrollment_id = ?', enrollment.enrollment_id),
    ctx.completedCount,
  );
  expect(
    "lesson_progress status='completed'",
    scalar(
      "SELECT COUNT(*) AS n FROM learning_lesson_progress WHERE enrollment_id = ? AND status = 'completed'",
      enrollment.enrollment_id,
    ),
    ctx.completedCount,
  );

  // due 翻转：settled 计数（app 口径 NOT EXISTS，xp.rs 同语义）与 due 标志同源
  const settled = scalar(
    "SELECT COUNT(*) AS n FROM learning_xp_events e \
     WHERE e.user_id = ? AND e.course_id = ? AND e.kind = 'node-completed' AND e.node_id IS NOT NULL \
       AND NOT EXISTS (SELECT 1 FROM learning_xp_events q \
         WHERE q.user_id = e.user_id AND q.course_id = e.course_id \
           AND q.kind = 'quiz-completed' AND q.rowid > e.rowid)",
    userId,
    courseId,
  );
  expect('quiz settled (since last quiz)', settled, ctx.completedCount);
  expect(
    'quiz due flip',
    settled >= QUIZ_TRIGGER_SETTLEMENTS ? 1 : 0,
    ctx.completedCount >= QUIZ_TRIGGER_SETTLEMENTS ? 1 : 0,
  );

  // 复习卡：本课名下（graph 照造不裁——不随 --settled 收缩）
  expect(
    'review cards',
    scalar('SELECT COUNT(*) AS n FROM learning_review_items WHERE enrollment_id = ?', enrollment.enrollment_id),
    ctx.cardCount,
  );

  if (ctx.isGraph) {
    expect(
      "concept hooks role='teaches'",
      scalar(
        `SELECT COUNT(*) AS n FROM learning_lesson_concepts WHERE lesson_id IN (${lessonIn}) AND role = 'teaches'`,
        ...lessonIds,
      ),
      GRAPH_LESSONS.filter((l) => l.concept).length,
    );
    expect(
      "concept hooks role='assumes'",
      scalar(
        `SELECT COUNT(*) AS n FROM learning_lesson_concepts WHERE lesson_id IN (${lessonIn}) AND role = 'assumes'`,
        ...lessonIds,
      ),
      GRAPH_ASSUMES.length,
    );
    expect(
      "growth batch status='applied'",
      scalar("SELECT COUNT(*) AS n FROM learning_growth_batches WHERE course_id = ? AND status = 'applied'", courseId),
      1,
    );
    // 软闸低 R 卡在场：stability ≤ 2d + 4 天内 push——只有供给节点卡满足
    expect(
      'supplier low-R card',
      scalar(
        'SELECT COUNT(*) AS n FROM learning_review_items ri \
         JOIN learning_activities a ON a.activity_id = ri.activity_id \
         JOIN learning_lessons l ON l.lesson_id = a.lesson_id \
         JOIN learning_modules m ON m.module_id = l.module_id \
         WHERE m.course_id = ? AND ri.stability_days > 0 AND ri.stability_days <= 2 \
           AND ri.last_reviewed_at IS NOT NULL AND ri.last_reviewed_at >= ?',
        courseId,
        ctx.now - 4 * DAY_MS,
      ),
      1,
    );
  }

  if (failures.length > 0) {
    console.error(`\nseed-learning-triggers: ${failures.length}/${checks} assertion(s) FAILED:`);
    for (const line of failures) console.error(line);
    process.exit(1);
  }
  console.log(`\nassertions: ${checks} checks passed`);
}

main();
