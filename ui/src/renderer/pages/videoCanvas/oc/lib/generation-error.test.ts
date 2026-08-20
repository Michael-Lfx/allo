import { describe, expect, test } from "bun:test";
import {
    CONTENT_MODERATION_ERROR_CODE,
    CONTENT_MODERATION_MESSAGE,
    COPYRIGHT_RESTRICTION_MESSAGE,
    REFERENCE_IMAGE_MODERATION_MESSAGE,
    generationErrorMessage,
    generationFailureMetadata,
    isContentModerationError,
    unchangedModeratedPrompt,
} from "./generation-error";

const copyrightWrappedAs500 = `Internal error: image generation failed: Image generation failed
Model: AIPC-doubao-seedream-5-0-lite-260128
Stage: image_generate
Cause: API error 500: Model call failed. Please try again later: InputTextSensitiveContentDetected.PolicyViolation: The request failed because the input text may be related to copyright restrictions Request id: 0217872058738352dc0f013dd9de882804ee1715bc98a4a7a9259
Hint: Check that the selected model is available, or resume from checkpoint later.`;

describe("generation-error", () => {
    test("maps copyright policy rejection wrapped as HTTP 500 to a friendly moderation message", () => {
        expect(isContentModerationError(copyrightWrappedAs500)).toBe(true);
        expect(generationErrorMessage(copyrightWrappedAs500)).toBe(COPYRIGHT_RESTRICTION_MESSAGE);
        expect(generationErrorMessage(copyrightWrappedAs500)).not.toContain("网络异常");

        const failure = generationFailureMetadata(new Error(copyrightWrappedAs500), "disney princess style");
        expect(failure.generationErrorCode).toBe(CONTENT_MODERATION_ERROR_CODE);
        expect(failure.errorDetails).toBe(COPYRIGHT_RESTRICTION_MESSAGE);
        expect(failure.failedPromptFingerprint).toBeTruthy();
        expect(unchangedModeratedPrompt(failure, "disney princess style")).toBe(true);
        expect(unchangedModeratedPrompt(failure, "original character")).toBe(false);
    });

    test("maps generic sensitive text rejection to content moderation message", () => {
        const raw = "API error 500: InputTextSensitiveContentDetected.PolicyViolation";
        expect(generationErrorMessage(raw)).toBe(CONTENT_MODERATION_MESSAGE);
        expect(generationFailureMetadata(raw, "prompt").generationErrorCode).toBe(CONTENT_MODERATION_ERROR_CODE);
    });

    test("maps reference-image privacy rejection to image moderation message", () => {
        const raw = "video failed: InputImageSensitiveContentDetected PrivacyInformation may contain real person";
        expect(generationErrorMessage(raw)).toBe(REFERENCE_IMAGE_MODERATION_MESSAGE);
    });

    test("keeps true network failures as network errors", () => {
        expect(generationErrorMessage("failed to fetch")).toBe("网络异常。");
        expect(generationErrorMessage("Request failed with status code 502 Bad Gateway")).toBe("网络异常。");
    });

    test("recognizes already-localized moderation messages for retry gating", () => {
        expect(isContentModerationError(COPYRIGHT_RESTRICTION_MESSAGE)).toBe(true);
        expect(isContentModerationError(CONTENT_MODERATION_MESSAGE)).toBe(true);
        expect(generationErrorMessage(COPYRIGHT_RESTRICTION_MESSAGE)).toBe(COPYRIGHT_RESTRICTION_MESSAGE);
    });
});
