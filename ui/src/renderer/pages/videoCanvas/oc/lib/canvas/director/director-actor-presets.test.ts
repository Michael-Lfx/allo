import { describe, expect, test } from "bun:test";

import { DIRECTOR_ACTOR_PRESETS, DIRECTOR_DEFAULT_ACTOR_URL, DIRECTOR_SOLDIER_ACTOR_URL, resolveDirectorActorPreset } from "./director-actor-presets";
import { createDirectorActor } from "./director-scene-create";

describe("director actor presets", () => {
    test("resolves four clay identities onto three.js sample GLBs", () => {
        expect(DIRECTOR_ACTOR_PRESETS.map((item) => item.id)).toEqual(["adult_male", "adult_female", "child", "elder"]);
        expect(resolveDirectorActorPreset("adult_male").url).toBe(DIRECTOR_SOLDIER_ACTOR_URL);
        expect(resolveDirectorActorPreset("adult_female").url).toBe(DIRECTOR_DEFAULT_ACTOR_URL);
        expect(resolveDirectorActorPreset("child").scale[1]).toBeLessThan(1);
        expect(resolveDirectorActorPreset("elder").pose).toBe("lean");
        expect(resolveDirectorActorPreset().id).toBe("adult_male");
    });

    test("createDirectorActor without preset stays Xbot at unit scale", () => {
        const actor = createDirectorActor("演员 1", [0, 0, 0]);
        expect(actor.url).toBe(DIRECTOR_DEFAULT_ACTOR_URL);
        expect(actor.actorPreset).toBeUndefined();
        expect(actor.transform.scale).toEqual([1, 1, 1]);
        expect(actor.pose).toBe("stand");
    });

    test("createDirectorActor stores the preset id, mesh, scale, and pose", () => {
        const child = createDirectorActor("儿童 1", [0, 0, 0], undefined, "child");
        expect(child.actorPreset).toBe("child");
        expect(child.url).toBe(DIRECTOR_DEFAULT_ACTOR_URL);
        expect(child.transform.scale).toEqual(resolveDirectorActorPreset("child").scale);
        expect(child.color).toBe(resolveDirectorActorPreset("child").color);
    });
});
