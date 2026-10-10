/**
 * Canvas Agent harness — policy overlay for the canvas observe–act loop.
 *
 * Same shape as `nomi-coding` / `CodingHarness`: constitution, advertised
 * tool surface, and completion gates. The generic turn driver stays in the
 * canvas UI host; this module does not share office/coding `AgentEngine`.
 */

import { CANVAS_AGENT_CODES } from "./canvas-agent-observation";
import { CANVAS_AGENT_READ_TOOLS, canvasAgentShouldContinueAfterTools, type CanvasAgentToolBatchOutcome } from "./canvas-agent-policy";

export { CANVAS_AGENT_READ_TOOLS };

export const CANVAS_AGENT_MAX_STEPS = 12;

export const CANVAS_AGENT_CONSTITUTION =
  "你是 allo 画布 Agent：对着节点图画布做感知—行动—观察，不是聊天机器人。每轮含[画布观察]和当前 scene（短 ID、几何、连线）。整理/去重叠/对齐：第一轮直接 canvas_apply（layout=true 或 patches.position 移动现有节点），不要先 canvas_inspect / canvas_propose。禁止 deleteIds 后按同样标题再建一套。制作短剧/短片/成片：空画布禁止 list_skills、list_templates、get_skill、*inspect、propose；立即 canvas_apply（简报 + 角色图 + script.shots 写满，run=true）。骨架不是完成——idle 图片必须 canvas_run，分镜有行但没有镜头视频就继续补。同一回合可并行多个工具。写入成功且目标已满足后停止，不要为口头总结再调用工具。可逆图操作会自动执行；生成与一次删除≥3 个节点才会等用户确认。优先用 storyboard_inspect / storyboard_apply 改现有分镜，spec_inspect / spec_apply 守规格，镜头模板用 canvas_apply_template。canvas_inspect 只在缺几何时深查。节点用短 ID（n1）或真实 id。自己根据用户目标设计图，不要套固定影视流水线；复用已有节点和选区。写给生图/视频模型的 prompt 只描述要看见的画面；外貌锁定写「同一张脸、同一套服装贯穿」，不要写「换脸」或堆「禁止xxx」（渠道会按字面审核）。队列未空或 idle 媒体未跑时绝不能说已完成。已在生成中、等待超时、或审核未过且提示词未改时，禁止再 canvas_run / repair rerun，也不要再弹出确认。需要用户选择时给出可点击短选项。";

export const CANVAS_AGENT_ADVERTISED_TOOLS = [
  "canvas_list_skills",
  "canvas_get_skill",
  "canvas_list_templates",
  "storyboard_inspect",
  "storyboard_apply",
  "subject_inspect",
  "spec_inspect",
  "spec_apply",
  "timeline_inspect",
  "canvas_inspect",
  "canvas_propose",
  "canvas_apply",
  "canvas_apply_template",
  "canvas_run",
  "canvas_critique",
  "canvas_repair",
] as const;

export type CanvasAgentAdvertisedTool = (typeof CANVAS_AGENT_ADVERTISED_TOOLS)[number];

export const CANVAS_AGENT_INCOMPLETE_NUDGE =
  `任务未完成（${CANVAS_AGENT_CODES.GOAL_INCOMPLETE}）。队列未空、媒体仍 idle，或分镜还没有成片节点。请 canvas_run 等待，或补镜头节点 / canvas_critique。不要对用户宣称完成。`;

export type CanvasAgentFinishDecision =
  | { action: "call_model"; toolChoice: "required" | "auto" }
  | { action: "force_tool"; nudge: string }
  | { action: "run_tools" }
  | { action: "await_confirm" }
  | { action: "end" }
  | { action: "hard_stop"; reason: "max_steps" };

export type CanvasHarnessConfig = {
  maxSteps?: number;
};

export class CanvasHarness {
  readonly maxSteps: number;

  constructor(config: CanvasHarnessConfig = {}) {
    this.maxSteps = config.maxSteps ?? CANVAS_AGENT_MAX_STEPS;
  }

  constitution() {
    return CANVAS_AGENT_CONSTITUTION;
  }

  advertiseTool(name: string) {
    return (CANVAS_AGENT_ADVERTISED_TOOLS as readonly string[]).includes(name);
  }

  isReadTool(name: string) {
    return CANVAS_AGENT_READ_TOOLS.has(name);
  }

  firstTurnToolChoice(): "required" {
    return "required";
  }

  decideAfterTools(step: number, outcome?: CanvasAgentToolBatchOutcome): Extract<CanvasAgentFinishDecision, { action: "call_model" | "end" | "hard_stop" }> {
    if (step >= this.maxSteps) return { action: "hard_stop", reason: "max_steps" };
    if (outcome && !canvasAgentShouldContinueAfterTools(outcome)) return { action: "end" };
    return { action: "call_model", toolChoice: "auto" };
  }

  decideAfterModel(input: {
    step: number;
    toolCallCount: number;
    writableCallCount: number;
    confirmTools: boolean;
    skipConfirm?: boolean;
    incomplete: boolean;
    needsConfirm?: boolean;
  }): CanvasAgentFinishDecision {
    if (input.toolCallCount > 0) {
      const confirm = input.needsConfirm ?? (input.confirmTools && !input.skipConfirm && input.writableCallCount > 0);
      if (confirm) return { action: "await_confirm" };
      return { action: "run_tools" };
    }
    if (input.incomplete && input.step + 1 < this.maxSteps) {
      return { action: "force_tool", nudge: CANVAS_AGENT_INCOMPLETE_NUDGE };
    }
    return { action: "end" };
  }
}

export const canvasHarness = new CanvasHarness();
