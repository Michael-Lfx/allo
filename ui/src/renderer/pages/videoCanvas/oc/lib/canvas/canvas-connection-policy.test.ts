import { describe, expect, test } from "bun:test";

import { defaultConfig } from "@oc/stores/use-config-store";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

import { canvasConnectionError } from "./canvas-connection-policy";

function node(id: string, type: CanvasNodeType): CanvasNodeData {
    return { id, type, title: id, position: { x: 0, y: 0 }, width: 100, height: 100 };
}

function videoConfig(videoModel: string) {
    const separator = videoModel.indexOf("::");
    const channelId = separator >= 0 ? videoModel.slice(0, separator) : "default";
    const modelName = separator >= 0 ? videoModel.slice(separator + 2) : videoModel;
    return {
        ...defaultConfig,
        videoModel,
        channels: [
            {
                ...defaultConfig.channels[0],
                id: channelId,
                models: [modelName],
            },
        ],
    };
}

describe("canvas video-to-video connections", () => {
    const source = node("src", CanvasNodeType.Video);
    const target = node("dst", CanvasNodeType.Video);
    const nodes = [source, target];
    const candidate = { fromNodeId: source.id, toNodeId: target.id };

    test("allows connecting a video node into a Seedance video node", () => {
        expect(canvasConnectionError(videoConfig("flowy::doubao-seedance-2-0"), nodes, [], candidate)).toBe("");
    });

    test("allows connecting a video node into a MiniMax-H3 video node", () => {
        expect(canvasConnectionError(videoConfig("flowy::MiniMax-H3"), nodes, [], candidate)).toBe("");
    });

    test("allows connecting a video node into a generic catalog video model", () => {
        expect(canvasConnectionError(videoConfig("default::grok-imagine-video"), nodes, [], candidate)).toBe("");
    });
});
