import { existsSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";

import { CRAFT_RECIPES } from "./recipes";
import { BUILTIN_PLAYBOOKS } from "./playbooks";
import { CRAFT_GRAPHS, GRAPH_BY_ID } from "./graphs";
import { expandRecipeTokens, insertRecipeToken, listedRecipeIds, removeRecipeToken, stripRecipeTokens } from "./tokens";
import { buildGraphStarter, recipeApplyPatch } from "./apply";
import { getAgentPlaybook, listAgentPlaybooks } from "./agent-catalog";
import { canvasPlaybookId, findPlaybook, mergeCraftAttachments, qualifiedPlaybookId, recipeFitsMedia, recipeMediaKind, recipesForMode } from "./catalog";
import { craftStillUrl } from "./covers";
import { canvasVideoSessionProps } from "./video-telemetry";

describe("craft catalog", () => {
    test("ships a full recipe shelf with covers and the original six", () => {
        expect(CRAFT_RECIPES.length).toBeGreaterThanOrEqual(40);
        for (const id of ["character-sheet", "multi-angle", "next-shot", "story-beats", "cinematic-light", "video-prompt"]) {
            expect(CRAFT_RECIPES.some((item) => item.id === id)).toBe(true);
        }
        expect(CRAFT_RECIPES.every((item) => item.coverLookId === item.id && item.prompt)).toBe(true);
        expect(recipesForMode("image").some((item) => item.id === "character-sheet")).toBe(true);
        expect(recipesForMode("audio").some((item) => item.id === "character-sheet")).toBe(false);
        expect(recipeMediaKind(CRAFT_RECIPES.find((item) => item.id === "character-sheet")!)).toBe("image");
        expect(recipeMediaKind(CRAFT_RECIPES.find((item) => item.id === "cam-push-in")!)).toBe("video");
        expect(recipeFitsMedia(CRAFT_RECIPES.find((item) => item.id === "character-sheet")!, "imageOnly")).toBe(true);
        expect(recipeFitsMedia(CRAFT_RECIPES.find((item) => item.id === "cam-push-in")!, "imageOnly")).toBe(false);
        expect(CRAFT_RECIPES.every((item) => craftStillUrl(item.coverLookId).includes(`/craft/${item.id}.png`))).toBe(true);
    });

    test("dedicated still files exist for every catalog cover id", () => {
        const dir = join(import.meta.dir, "../../../../../../../../public/craft");
        const ids = [
            ...CRAFT_RECIPES.map((item) => item.coverLookId),
            ...BUILTIN_PLAYBOOKS.map((item) => item.coverLookId),
            ...CRAFT_GRAPHS.map((item) => item.coverLookId),
            "cinematic",
        ];
        for (const id of ids) {
            expect(existsSync(join(dir, `${id}.png`)), id).toBe(true);
        }
    });

    test("playbooks include vimax builtins and canvas-native manuals", () => {
        expect(BUILTIN_PLAYBOOKS).toHaveLength(16);
        expect(BUILTIN_PLAYBOOKS.every((item) => item.qualifiedId.startsWith("builtin:"))).toBe(true);
        expect(BUILTIN_PLAYBOOKS.some((item) => item.id === "short-drama")).toBe(true);
        expect(BUILTIN_PLAYBOOKS.some((item) => item.id === "character-bible")).toBe(true);
        expect(findPlaybook("cinematic")).toBeUndefined();
        expect(findPlaybook("builtin:short-drama")?.id).toBe("short-drama");
    });

    test("recipe tokens stay hidden from visible text and expand at generate", () => {
        const withToken = insertRecipeToken("角色走过来", "next-shot");
        expect(listedRecipeIds(withToken)).toEqual(["next-shot"]);
        expect(stripRecipeTokens(withToken)).toBe("角色走过来");
        expect(expandRecipeTokens(withToken)).toContain("下一个连续镜头");
        expect(removeRecipeToken(withToken, "next-shot")).toBe("角色走过来");
        expect(mergeCraftAttachments("角色走过来", ["next-shot"], ["builtin:short-drama"])).toContain("@[recipe:next-shot]");
        expect(mergeCraftAttachments("角色走过来", ["next-shot"], ["builtin:short-drama"])).toContain("@[skill:builtin:short-drama]");
        expect(stripRecipeTokens(mergeCraftAttachments("角色走过来", ["next-shot"], []))).toBe("角色走过来");
        expect(listedRecipeIds(mergeCraftAttachments("hello @[recipe:next-shot]", [], []))).toEqual(["next-shot"]);
        const camera = CRAFT_RECIPES.find((item) => item.id === "cam-push-in")!;
        expect(recipeApplyPatch(camera, "").cameraMoveId).toBe("push_in");
    });

    test("look ids are not playbooks", () => {
        expect(findPlaybook("cinematic")).toBeUndefined();
        expect(qualifiedPlaybookId("cinematic")).toBeUndefined();
        expect(qualifiedPlaybookId("builtin:short-drama")).toBe("builtin:short-drama");
        expect(canvasPlaybookId([{ metadata: { projectPlaybook: true, skillId: "builtin:short-drama" } }])).toBe("builtin:short-drama");
        const props = canvasVideoSessionProps("proj-1", [{ metadata: { projectPlaybook: true, skillId: "builtin:short-drama" } }]);
        expect(props.workflow).toBe("canvas");
        expect(props.session_id).toBe("proj-1");
        expect(props.playbook_id).toBe("builtin:short-drama");
        expect(canvasVideoSessionProps("proj-2", []).playbook_id).toBeUndefined();
    });

    test("graph starters return connected nodes", () => {
        expect(CRAFT_GRAPHS).toHaveLength(8);
        const built = buildGraphStarter("starter-script-film", { x: 400, y: 200 });
        expect(built?.nodes).toHaveLength(3);
        expect(built?.connections).toHaveLength(2);
        expect(GRAPH_BY_ID.get("starter-character")).toBeTruthy();
    });

    test("agent catalog lists builtins even on empty canvas", () => {
        const listed = listAgentPlaybooks([]);
        expect(listed.length).toBe(16);
        expect(getAgentPlaybook([], "builtin:short-drama")?.instruction).toContain("钩子");
        expect(getAgentPlaybook([], "", "角色圣经")?.skillId).toBe("builtin:character-bible");
    });
});
