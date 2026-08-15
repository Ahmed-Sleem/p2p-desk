import { describe, expect, it } from "vitest";
import { normalizeAppError } from "./client";

describe("normalizeAppError", () => {
  it("retains a valid typed core error", () => {
    expect(
      normalizeAppError({
        code: "CORE-PREREQUISITE",
        category: "prerequisite",
        message: "WebView2 is required.",
        retryable: false,
        requestId: null,
      }),
    ).toEqual({
      code: "CORE-PREREQUISITE",
      category: "prerequisite",
      message: "WebView2 is required.",
      retryable: false,
      requestId: null,
    });
  });

  it.each([null, "failure", {}, { code: 3 }])(
    "fails closed for malformed error value %j",
    (value) => {
      expect(normalizeAppError(value)).toMatchObject({
        code: "CORE-UNKNOWN",
        retryable: false,
      });
    },
  );
});
