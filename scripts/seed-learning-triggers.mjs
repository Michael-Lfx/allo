#!/usr/bin/env bun
/**
 * Seed the dev-channel database with learning data that triggers the review
 * (复习) and quiz (测验) features immediately, for pre-launch verification.
 *
 * Why: review cards are first due at the NEXT review day's 02:00 local
 * (first_review_due_at), and a quiz unlocks only after 6 settled nodes
 * (QUIZ_TRIGGER_SETTLEMENTS) — both need real time / real accumulation on a
 * clean dev database. This script fabricates the accumulated state directly:
 *
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
 * Deliberately NOT seeded: `quiz-completed` events (keeps quiz due), custom
 * questions, today's checkin.
 *
 * Known divergence (by design): fabricated settlements bypass the companion
 * XP bridge, so the companion balance does not see this history — the
 * learning ledger surfaces (calendar / checkin / triggers) are unaffected.
 *
 * SAFETY: close ALL app instances (desktop, `bun run dev`, `dev:web`,
 * `nomicore`) first — the backup step (`VACUUM INTO`) doubles as the lock
 * probe and fails fast on a live database. Dev channel only: the default
 * target is the `Nomi-dev` data dir; pass `--db` to aim elsewhere.
 *
 * Usage:
 *   bun scripts/seed-learning-triggers.mjs [--db <path>] [--fresh] [--dry-run]
 *     --db       target SQLite path (default: dev channel data dir)
 *     --fresh    delete the previous launch-sim course's derived rows, reseed
 *     --dry-run  print the plan without writing anything
 *
 * Idempotent: reruns detect the marker course and skip; `--fresh` rebuilds.
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
/** Marker course title — idempotency + --fresh cleanup anchor. */
const MARKER_TITLE = 'launch-sim 触发数据';

// ---------------------------------------------------------------------------
// argv
// ---------------------------------------------------------------------------

function parseArgs(argv) {
  const args = { db: null, fresh: false, dryRun: false };
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
    } else {
      throw new Error(`unknown argument: ${value}`);
    }
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
  const dbPath = args.db ?? defaultDevDb();
  if (!existsSync(dbPath)) {
    fail(`database not found: ${dbPath} (pass --db to aim elsewhere)`);
  }
  const stat = statSync(dbPath);
  if (!stat.isFile()) fail(`target is not a file: ${dbPath}`);

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

  const marker = db.query('SELECT course_id FROM learning_courses WHERE title = ?').get(MARKER_TITLE);
  if (marker && !args.fresh) {
    console.log(`marker course exists (${marker.course_id}); nothing to do (pass --fresh to rebuild)`);
    db.close();
    return;
  }

  if (args.dryRun) {
    console.log('\n[dry-run] plan:');
    console.log(`  course        1   (${MARKER_TITLE})`);
    console.log('  module        1');
    console.log(`  lessons       ${LESSONS.length} (est ${NODE_EST_MINUTES}, content_generated=1)`);
    console.log(`  activities    ${LESSONS.length} (${[...new Set(LESSONS.map((l) => l.kind))].join(', ')})`);
    console.log('  enrollment    1');
    console.log(`  xp events     ${LESSONS.length} node-completed, days 4-7 ago, 54 XP/day`);
    console.log(`  review items  ${CARD_PLANS.length} (4 bare overdue + 4 with push history + review_log)`);
    console.log('  checkins      4 past days (today left unlocked)');
    db.close();
    return;
  }

  // ---- backup (also the lock probe: a running backend fails here) ----------
  const stamp = new Date(now).toISOString().replace(/[-:T]/g, '').slice(0, 14);
  const backupPath = `${dbPath}.pre-seed-learning-${stamp}`;
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
    db.run(
      'INSERT INTO learning_courses (course_id, title, description, domain, version, created_at, updated_at) \
       VALUES (?, ?, ?, ?, 1, ?, ?)',
      courseId,
      MARKER_TITLE,
      '由 scripts/seed-learning-triggers.mjs 造数生成，用于上线前复习/测验触发验证；验证完可整课删除。',
      'general',
      now,
      now,
    );
    db.run(
      'INSERT INTO learning_modules (module_id, course_id, title, description, position) \
       VALUES (?, ?, ?, ?, 0)',
      moduleId,
      courseId,
      '模拟模块',
      '',
    );
    const lessonIds = [];
    const activityIds = [];
    LESSONS.forEach((lesson, index) => {
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

    // ---- settlement history: node-completed, 2 per day, days 4..7 ago -----
    const xpDays = []; // { ts, xp } per day for the checkin rows
    lessonIds.forEach((lessonId, index) => {
      const daysAgo = 4 + Math.floor(index / 2); // 8 nodes → 4 days × 2 nodes
      const ts = localMoment(daysAgo, 10, 5 + (index % 2) * 95 + index * 7);
      const earned = NODE_EST_MINUTES + XP_FULL_BONUS; // settle_node(full): est + bonus
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
    let logCount = 0;
    CARD_PLANS.forEach((plan, index) => {
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
    console.log(`seeded: course ${courseId}, ${lessonIds.length} lessons, ${lessonIds.length} xp events, ${CARD_PLANS.length} cards, ${logCount} log rows, ${checkinCount} checkins`);
    printReadout(db, owner.user_id, courseId, now, tz);
  } catch (error) {
    db.exec('ROLLBACK');
    fail(error.message);
  }
  db.close();
}

/** --fresh: remove every row derived from the previous seed run. Checkins are
 * matched by the seed fingerprint (goal + reviewed_count) on the seeded days
 * so real checkins on the same days survive. */
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
  // checkins: only rows carrying the seed fingerprint (goal 40, reviewed_count 54)
  db.run(
    'DELETE FROM learning_checkins WHERE user_id = ? AND goal = ? AND reviewed_count = ?',
    userId,
    DAILY_XP_GOAL_DEFAULT,
    (NODE_EST_MINUTES + XP_FULL_BONUS) * 2,
  );
  if (enrollmentIds.length) db.run(`DELETE FROM learning_enrollments WHERE enrollment_id IN (${enrollmentIds.map(() => '?').join(',')})`, ...enrollmentIds);
  if (lessonIds.length) {
    db.run(`DELETE FROM learning_activities WHERE lesson_id IN (${lessonIds.map(() => '?').join(',')})`, ...lessonIds);
    db.run(`DELETE FROM learning_lessons WHERE lesson_id IN (${lessonIds.map(() => '?').join(',')})`, ...lessonIds);
  }
  db.run('DELETE FROM learning_modules WHERE course_id = ?', courseId);
  db.run('DELETE FROM learning_courses WHERE course_id = ?', courseId);
}

/** App-口径 readout: the same SQL shapes the backend uses. */
function printReadout(db, userId, courseId, now, tz) {
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
  console.log('\nnext steps (真机走查见 docs/tracker/learning-launch-sim/tickets/s1-seed-learning-triggers.md):');
  console.log(`  1. start the dev app and verify the review surface`);
  console.log(`  2. quiz API: GET /api/learning/courses/${courseId}/quiz/progress`);
}

main();
