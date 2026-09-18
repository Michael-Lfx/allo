import { describe, expect, test } from "bun:test";

import {
  resolveVideoImageReferences,
  shouldSubmitVideoImagesAsReferences,
  videoFramePatchFromStillRole,
} from "./video-reference-roles";

describe("video still roles", () => {
  test("named first+last on two stills stay image_to_video", () => {
    expect(shouldSubmitVideoImagesAsReferences({
      videoEditOperation: "image_to_video",
      videoStartFrameNodeId: "a",
      videoEndFrameNodeId: "b",
    }, 2)).toBe(false);
    expect(resolveVideoImageReferences(
      [{ id: "a" }, { id: "b" }],
      { videoStartFrameNodeId: "a", videoEndFrameNodeId: "b" },
    )).toEqual([
      { image: { id: "a" }, role: "first_frame" },
      { image: { id: "b" }, role: "last_frame" },
    ]);
  });

  test("three unlabeled stills all become reference_image", () => {
    expect(shouldSubmitVideoImagesAsReferences({}, 3)).toBe(true);
    expect(resolveVideoImageReferences([{ id: "a" }, { id: "b" }, { id: "c" }])).toEqual([
      { image: { id: "a" }, role: "reference_image" },
      { image: { id: "b" }, role: "reference_image" },
      { image: { id: "c" }, role: "reference_image" },
    ]);
  });

  test("videoFramePatchFromStillRole maps named roles without inventing a count fork", () => {
    expect(videoFramePatchFromStillRole("img-1", "first")).toEqual({
      videoEditOperation: "image_to_video",
      videoStartFrameNodeId: "img-1",
      videoEndFrameNodeId: undefined,
    });
    expect(videoFramePatchFromStillRole("img-1", "last")).toEqual({
      videoEditOperation: "image_to_video",
      videoStartFrameNodeId: undefined,
      videoEndFrameNodeId: "img-1",
    });
    expect(videoFramePatchFromStillRole("img-1", "reference")).toEqual({
      videoEditOperation: "reference_to_video",
      videoStartFrameNodeId: undefined,
      videoEndFrameNodeId: undefined,
    });
    expect(videoFramePatchFromStillRole(undefined, "first")).toEqual({});
  });
});
