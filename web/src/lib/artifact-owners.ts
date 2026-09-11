import type { RunPlan } from "./protocol";

/**
 * R20a：产物 → 所属 Run / Step 的映射（补 W5「按 Run 归属」这个缺口）。
 *
 * 数据只来自**已加载的 `run/plan` 快照**——`attempt.output_files` 本身就是权威归属
 * （`nomifun-agent-execution/src/runtime_adapter.rs` 的 `run_plan()` 已把它投影到
 * wire），所以这里既**不需要新协议能力**，也**不需要给宿主面文件服务再开一层
 * 映射**：纯客户端投影，零协议增量、零新端点、零新权限面。
 *
 * 有意保留的边界（不猜、不补占位）：
 * - 只能归属**本会话里加载过计划快照的那些 Run**。没看过计划的 Run，其产物显示为
 *   「无归属」，界面不摆空标签；
 * - 路径匹配只做无歧义归一化（见 `normalizeArtifactKey`），宁可漏认也不误认。
 */

export interface ArtifactOwner {
  runId: string;
  stepId: string;
  /** 步骤标题；快照缺标题时回落 step id（与 `planStepViews` 同口径）。 */
  stepTitle: string;
  attemptNo: number;
}

/**
 * 按会话累积的归属表。`conversationId` 是这份 `byPath` 属于哪个会话——界面只在
 * 它与当前选中会话一致时才使用，避免把上一个会话的归属贴到新会话的产物上。
 */
export interface ArtifactOwnerIndex {
  conversationId: string | null;
  byPath: Record<string, ArtifactOwner>;
}

/**
 * 归属匹配用的键：把快照里的产物路径与列表里的 `relative_path` 归一到同一形态。
 *
 * 只做**无歧义**的归一化——去首尾空白、去掉 `./` 前缀与前导分隔符、`\` → `/`
 * （Windows 运行时可能给出反斜杠）。**不做大小写折叠**：那会在区分大小写的平台上
 * 制造错误的归属。空路径返回 `null`（不参与匹配）。
 */
export function normalizeArtifactKey(path: string | null | undefined): string | null {
  if (typeof path !== "string") return null;
  let key = path.trim().replace(/\\/g, "/");
  while (key.startsWith("./")) key = key.slice(2);
  key = key.replace(/^\/+/, "");
  return key.length > 0 ? key : null;
}

/**
 * 快照 → 归属表。
 *
 * 同一条路径出现在多次尝试里时**保留最先出现的那次**：产物是它先产出的，后续尝试
 * 只是复现；覆盖成最后一次会让「谁产出的」变成「谁最后碰过」。
 */
export function collectArtifactOwners(
  plan: RunPlan | null | undefined,
  runId: string,
): Record<string, ArtifactOwner> {
  const owners: Record<string, ArtifactOwner> = {};
  if (!plan?.steps?.length || !runId) return owners;

  for (const step of plan.steps) {
    const stepTitle = step.title?.trim() || step.step_id;
    for (const attempt of step.attempts ?? []) {
      for (const file of attempt.output_files ?? []) {
        const key = normalizeArtifactKey(file);
        if (key === null || key in owners) continue;
        owners[key] = {
          runId,
          stepId: step.step_id,
          stepTitle,
          attemptNo: attempt.attempt_no,
        };
      }
    }
  }

  return owners;
}
