import { describe, expect, test } from "bun:test";

import { CanvasNodeType } from "@oc/types/canvas";
import { createStoryboardRow } from "./canvas-project-domain";
import type { CanvasAgentSnapshot } from "./canvas-agent-ops";
import {
  compileSpecApplyOps,
  compileStoryboardApplyOps,
  inspectArgsForCreationTool,
} from "./creation-agent-intent";

function snapshot(): CanvasAgentSnapshot {
  const row = createStoryboardRow(1, {
    id: "row-1",
    plotDescription: "出门",
    durationSeconds: 6,
    stillRole: "first",
  });
  return {
    projectId: "p1",
    title: "画布",
    nodes: [
      {
        id: "script-1",
        type: CanvasNodeType.Script,
        title: "分镜脚本",
        position: { x: 0, y: 0 },
        width: 920,
        height: 360,
        metadata: {
          storyboard: {
            rows: [row],
            visibleColumns: ["shotNumber", "durationSeconds", "plotDescription", "dialogue"],
            referenceNodeIds: [],
          },
        },
      },
      {
        id: "cfg-1",
        type: CanvasNodeType.Config,
        title: "规格",
        position: { x: 0, y: 400 },
        width: 280,
        height: 180,
        metadata: { size: "16:9", vquality: "1080p", seconds: "6", generationMode: "video" },
      },
    ],
    connections: [],
    selectedNodeIds: [],
    viewport: { x: 0, y: 0, k: 1 },
  };
}

describe("creation agent intent", () => {
  test("inspectArgsForCreationTool maps domain tools onto inspect focus", () => {
    expect(inspectArgsForCreationTool("storyboard_inspect", { query: "出门" })).toEqual({
      query: "出门",
      focus: "storyboard",
      types: ["script"],
    });
    expect(inspectArgsForCreationTool("subject_inspect", {})).toEqual({ focus: "subjects" });
    expect(inspectArgsForCreationTool("spec_inspect", {})).toEqual({ focus: "spec" });
    expect(inspectArgsForCreationTool("timeline_inspect", {})).toEqual({ focus: "timeline" });
    expect(inspectArgsForCreationTool("canvas_inspect", { focus: "graph" })).toEqual({ focus: "graph" });
  });

  test("compileStoryboardApplyOps patches the existing Script rows", () => {
    const ops = compileStoryboardApplyOps({
      shots: [{ id: "row-1", plot: "客厅起舞", durationSecs: 8, stillRole: "last" }],
    }, snapshot());
    expect(ops).toHaveLength(1);
    expect(ops[0]).toMatchObject({ type: "update_node", id: "script-1" });
    const rows = ops[0] && ops[0].type === "update_node"
      ? (ops[0].metadata as { storyboard?: { rows: Array<{ id: string; plotDescription: string; durationSeconds: number; stillRole?: string }> } } | undefined)?.storyboard?.rows
      : undefined;
    expect(rows).toHaveLength(1);
    expect(rows?.[0]).toMatchObject({
      id: "row-1",
      plotDescription: "客厅起舞",
      durationSeconds: 8,
      stillRole: "last",
    });
  });

  test("compileStoryboardApplyOps refuses to run without a Script node", () => {
    const empty = { ...snapshot(), nodes: snapshot().nodes.filter((node) => node.type !== CanvasNodeType.Script) };
    expect(() => compileStoryboardApplyOps({ shots: [{ plot: "空" }] }, empty)).toThrow("没有分镜脚本节点");
  });

  test("compileSpecApplyOps writes named fields onto the Config node", () => {
    const ops = compileSpecApplyOps({ aspectRatio: "9:16", durationSecs: 10, model: "demo" }, snapshot());
    expect(ops).toEqual([{
      type: "update_node",
      id: "cfg-1",
      metadata: { size: "9:16", seconds: "10", model: "demo" },
    }]);
  });
});
