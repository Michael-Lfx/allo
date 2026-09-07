import { describe, expect, test } from "bun:test";

import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";
import {
  buildCreationIrFromLaunch,
  hydrateCreationIr,
  isCreationShotBusy,
  parseCreationIr,
  parseCreationView,
  readCreationIr,
  readCreationMemory,
  resolveCreationIr,
  summarizeCreationForAgent,
  workflowKindForSubject,
  writeCreationIr,
} from "./creation-ir";

describe("creation IR", () => {
  test("parses a v1 document and rejects unknown schemas", () => {
    const ir = parseCreationIr({
      schema: 1,
      view: "storyboard",
      spec: { aspectRatio: "9:16", resolution: "720p", durationSecs: 6, mediaKind: "video" },
      subjects: [{ id: "sub_1", kind: "character", name: "噜噜", mediaId: "m1" }],
      shots: [{ id: "shot-1", plot: "跳舞", durationSecs: 6, subjectIds: ["sub_1"] }],
    });
    expect(ir?.spec.aspectRatio).toBe("9:16");
    expect(ir?.subjects[0]?.name).toBe("噜噜");
    expect(parseCreationIr({ schema: 2, spec: ir?.spec, subjects: [], shots: [] })).toBeNull();
  });

  test("parses still roles, timeline view, and shot busy status", () => {
    const ir = parseCreationIr({
      schema: 1,
      view: "timeline",
      spec: { aspectRatio: "9:16", resolution: "720p", durationSecs: 6, mediaKind: "video" },
      subjects: [],
      shots: [{ id: "shot-1", plot: "跳舞", durationSecs: 6, subjectIds: [], stillRole: "last", status: "running" }],
    });
    expect(ir?.view).toBe("timeline");
    expect(ir?.shots[0]?.stillRole).toBe("last");
    expect(parseCreationView("canvas")).toBe("canvas");
    expect(parseCreationView("nope")).toBe("storyboard");
    expect(isCreationShotBusy("running")).toBe(true);
    expect(isCreationShotBusy("idle")).toBe(false);
  });

  test("buildCreationIrFromLaunch seeds one draft shot bound to uploaded subjects", () => {
    const ir = buildCreationIrFromLaunch({
      prompt: "噜噜在客厅跳舞",
      mediaKind: "video",
      preferences: { aspectRatio: "16:9", resolution: "1080p", targetDurationSecs: 8, videoModel: "demo" },
      skill: { id: "cinematic", label: "电影写实" },
      subjects: [
        { kind: "character", name: "噜噜", mediaId: "media-a" },
        { kind: "scene", name: "客厅", mediaId: "media-b" },
      ],
    });
    expect(ir.view).toBe("storyboard");
    expect(ir.subjects.map((item) => item.kind)).toEqual(["character", "scene"]);
    expect(ir.shots).toHaveLength(1);
    expect(ir.shots[0]?.plot).toBe("噜噜在客厅跳舞");
    expect(ir.shots[0]?.subjectIds).toEqual(["sub_media-a", "sub_media-b"]);
    expect(ir.spec.durationSecs).toBe(8);
    expect(ir.skill?.name).toBe("电影写实");
    expect(workflowKindForSubject("character")).toBe("character");
    expect(workflowKindForSubject("prop")).toBe("reference_set");
  });

  test("hydrate prefers live script rows and labeled image nodes", () => {
    const stored = buildCreationIrFromLaunch({
      prompt: "草稿",
      mediaKind: "video",
      preferences: { aspectRatio: "16:9", resolution: "1080p", targetDurationSecs: 5 },
      subjects: [{ id: "sub_media-a", kind: "character", name: "噜噜", mediaId: "media-a" }],
    });
    const nodes: CanvasNodeData[] = [
      {
        id: "img-1",
        type: CanvasNodeType.Image,
        title: "噜噜",
        position: { x: 0, y: 0 },
        width: 160,
        height: 160,
        metadata: { workflowKind: "character", characterName: "噜噜", assetId: "media-a", status: "success" },
      },
      {
        id: "script-1",
        type: CanvasNodeType.Script,
        title: "分镜脚本",
        position: { x: 0, y: 0 },
        width: 920,
        height: 360,
        metadata: {
          storyboard: {
            rows: [{
              id: "row-1",
              shotNumber: 1,
              durationSeconds: 6,
              plotDescription: "客厅起舞",
              dialogue: "",
              characters: [{ characterName: "噜噜", characterImageNodeId: "img-1" }],
              narrativeIntent: "",
              viewerPOV: "",
              performanceBlocking: "",
              shotSize: "",
              emotion: "",
              lightingAndAtmosphere: "",
              audioEffects: "",
              camera: "",
              motion: "",
              timeBeats: "",
              imageGenerationPrompt: "",
              videoMotionPrompt: "",
              mustHave: [],
              optionalDetails: [],
              continuityOut: "",
              negativePrompt: "",
              referenceNodeIds: ["img-1"],
              imageNodeId: "kf-1",
              stillRole: "first",
              status: "idle",
            }],
            visibleColumns: ["shotNumber", "durationSeconds", "plotDescription", "dialogue"],
            referenceNodeIds: ["img-1"],
          },
        },
      },
    ];
    const live = hydrateCreationIr(stored, nodes);
    expect(live?.subjects[0]?.nodeId).toBe("img-1");
    expect(live?.shots[0]?.plot).toBe("客厅起舞");
    expect(live?.shots[0]?.imageNodeId).toBe("kf-1");
    expect(live?.shots[0]?.stillRole).toBe("first");
    expect(live?.shots[0]?.subjectIds).toContain("sub_media-a");
  });

  test("read/write sidecar and agent summary stay bounded", () => {
    const ir = buildCreationIrFromLaunch({
      prompt: "小猫出门",
      mediaKind: "video",
      preferences: { aspectRatio: "16:9", resolution: "1080p", targetDurationSecs: 5 },
    });
    const creative = writeCreationIr({ homeLaunch: { schema: 1 } }, ir);
    expect(readCreationIr(creative)?.shots[0]?.plot).toBe("小猫出门");
    expect(readCreationMemory(creative)).toHaveLength(1);
    const sameFingerprint = writeCreationIr(creative, { ...ir, view: "canvas" });
    expect(readCreationMemory(sameFingerprint)).toHaveLength(1);
    const nextSpec = writeCreationIr(sameFingerprint, { ...ir, spec: { ...ir.spec, durationSecs: 12 } });
    expect(readCreationMemory(nextSpec)).toHaveLength(2);
    expect(readCreationMemory(nextSpec).at(-1)?.spec.durationSecs).toBe(12);
    const summary = summarizeCreationForAgent(resolveCreationIr(creative, []));
    expect(summary?.rules.some((rule) => rule.includes("visual slot"))).toBe(true);
    expect(summary?.spec.aspectRatio).toBe("16:9");
    expect(summary?.gaps).toContain("shot shot-draft-1 is missing still");
    expect(summary?.gaps).toContain("shot shot-draft-1 is missing video");
  });

  test("resolveCreationIr ignores generate canvases without stored IR", () => {
    const nodes: CanvasNodeData[] = [{
      id: "script-1",
      type: CanvasNodeType.Script,
      title: "分镜脚本",
      position: { x: 0, y: 0 },
      width: 920,
      height: 360,
      metadata: { storyboard: { rows: [], visibleColumns: [], referenceNodeIds: [] } },
    }];
    expect(resolveCreationIr({ homeLaunch: { intent: "generate", prompt: "clip" } }, nodes)).toBeNull();
    expect(resolveCreationIr({}, nodes)).toBeNull();
  });
});
