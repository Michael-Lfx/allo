import { describe, expect, test } from "bun:test";

import { formatCanvasUserError } from "./canvas-user-error";

describe("formatCanvasUserError", () => {
  test("maps art critique pipeline codes to a user-readable sentence", () => {
    expect(formatCanvasUserError(new Error("art_critique_pipeline_failed"))).toContain("画面分析未完成");
    expect(formatCanvasUserError("review_art_composition_missing")).toContain("没有按约定返回");
    expect(formatCanvasUserError("art_critique_candidates_invalid")).toContain("不完整");
    expect(formatCanvasUserError("art_critique_pipeline_failed")).not.toContain("art_critique_pipeline_failed");
  });

  test("maps abort, vision-model, and tool_choice failures", () => {
    const abort = new DOMException("Aborted", "AbortError");
    expect(formatCanvasUserError(abort)).toBe("已取消");
    expect(formatCanvasUserError(new Error("未配置支持图片理解的文本模型（模型 extra.input 需包含 image）"))).toContain("不支持理解图片");
    expect(formatCanvasUserError(new Error("HTTP 400: tool_choice is not supported"))).toContain("不支持这种调用方式");
  });

  test("does not leak snake_case or HTTP dumps", () => {
    expect(formatCanvasUserError("analyze_art_scene_invalid_json")).not.toMatch(/_/);
    expect(formatCanvasUserError("foo_bar_internal_code")).toBe("操作失败，请稍后重试。");
    expect(formatCanvasUserError("<!DOCTYPE html><html>bad gateway</html>")).toBe("网络异常。");
  });

  test("keeps already friendly Chinese text", () => {
    expect(formatCanvasUserError(new Error("当前节点没有可转写的本地媒体"))).toBe("当前节点没有可转写的本地媒体");
  });
});
