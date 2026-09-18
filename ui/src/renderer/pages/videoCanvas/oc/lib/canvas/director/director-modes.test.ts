import { describe, expect, test } from "bun:test";

import { DIRECTOR_ADVANCED_MODES, DIRECTOR_BLOCKING_MODES, DIRECTOR_DEFAULT_MODE, directorModeCapabilities, isDirectorAdvancedMode, isDirectorBlockingMode } from "./director-modes";

describe("director modes", () => {
    test("blocking path is layout + camera; pose/animate stay advanced", () => {
        expect(DIRECTOR_DEFAULT_MODE).toBe("layout");
        expect([...DIRECTOR_BLOCKING_MODES]).toEqual(["layout", "camera"]);
        expect([...DIRECTOR_ADVANCED_MODES]).toEqual(["pose", "animate"]);
        expect(isDirectorBlockingMode("layout")).toBe(true);
        expect(isDirectorBlockingMode("camera")).toBe(true);
        expect(isDirectorAdvancedMode("pose")).toBe(true);
        expect(isDirectorAdvancedMode("animate")).toBe(true);
        expect(directorModeCapabilities("layout").timeline).toBe(false);
        expect(directorModeCapabilities("animate").timeline).toBe(true);
    });
});
