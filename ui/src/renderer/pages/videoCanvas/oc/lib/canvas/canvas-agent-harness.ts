/**
 * Canvas Agent harness — policy overlay for the canvas observe–act loop.
 *
 * Same shape as `nomi-coding` / `CodingHarness`: constitution, advertised
 * tool surface, and completion gates. The generic turn driver stays in the
 * canvas UI host; this module does not share office/coding `AgentEngine`.
 */

import { CANVAS_AGENT_CODES } from "./canvas-agent-observation";

export const CANVAS_AGENT_MAX_STEPS = 24;

export const CANVAS_AGENT_CONSTITUTION =
  "你是 allo 画布 Agent：对着节点图画布做感知—行动—观察，不是聊天机器人。每轮消息含最新[画布观察]（fingerprint、NEW/MODIFIED、生成队列）。用意图工具：canvas_inspect 深查节点/资源；canvas_propose 只出计划不写图；canvas_apply 按你为这个用户目标设计的 nodes+edges 更新画布（必须带节点，禁止只传 description；不是固定流水线；编译器会补 @ 引用、多图参考、时长）。canvas_run 提交生成并等待指定节点；canvas_critique 根据真实产物找问题；canvas_repair 按错误码修补或只重跑失败节点。节点用短 ID（n1）或真实 id。自己根据用户目标设计图，不要套固定模板；复用已有节点和选区。队列未空或资源未就绪时绝不能说已完成。技能先 canvas_get_skill 再按契约执行。需要用户选择时给出可点击短选项。";

export const CANVAS_AGENT_ADVERTISED_TOOLS = [
  "canvas_list_skills",
  "canvas_get_skill",
  "canvas_inspect",
  "canvas_propose",
  "canvas_apply",
  "canvas_run",
  "canvas_critique",
  "canvas_repair",
] as const;

export type CanvasAgentAdvertisedTool = (typeof CANVAS_AGENT_ADVERTISED_TOOLS)[number];

export const CANVAS_AGENT_READ_TOOLS = new Set<string>([
  "canvas_list_skills",
  "canvas_get_skill",
  "canvas_inspect",
  "canvas_propose",
  "canvas_critique",
]);

export const CANVAS_AGENT_INCOMPLETE_NUDGE =
  `生成队列未空（${CANVAS_AGENT_CODES.GOAL_INCOMPLETE}）。请 canvas_run 等待或 canvas_critique/canvas_repair，不要对用户宣称完成。`;

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

  decideAfterTools(step: number): CanvasAgentFinishDecision {
    if (step >= this.maxSteps) return { action: "hard_stop", reason: "max_steps" };
    return { action: "call_model", toolChoice: "auto" };
  }

  decideAfterModel(input: {
    step: number;
    toolCallCount: number;
    writableCallCount: number;
    confirmTools: boolean;
    skipConfirm?: boolean;
    incomplete: boolean;
  }): CanvasAgentFinishDecision {
    if (input.toolCallCount > 0) {
      if (input.confirmTools && !input.skipConfirm && input.writableCallCount > 0) {
        return { action: "await_confirm" };
      }
      return { action: "run_tools" };
    }
    if (input.incomplete && input.step + 1 < this.maxSteps) {
      return { action: "force_tool", nudge: CANVAS_AGENT_INCOMPLETE_NUDGE };
    }
    return { action: "end" };
  }
}

export const canvasHarness = new CanvasHarness();
