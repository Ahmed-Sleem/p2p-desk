import { invoke } from "@tauri-apps/api/core";
import type { AppErrorEnvelope, BootstrapInfo } from "./contracts";

export interface CoreClient {
  getBootstrapInfo(): Promise<BootstrapInfo>;
}

export const coreClient: CoreClient = {
  getBootstrapInfo: () => invoke<BootstrapInfo>("get_bootstrap_info"),
};

function isAppErrorCategory(
  value: unknown,
): value is AppErrorEnvelope["category"] {
  return (
    value === "configuration" ||
    value === "prerequisite" ||
    value === "storage" ||
    value === "provider" ||
    value === "lifecycle" ||
    value === "internal"
  );
}

const UNKNOWN_ERROR: AppErrorEnvelope = {
  code: "CORE-UNKNOWN",
  category: "internal",
  message: "The trusted core returned an unrecognized error.",
  retryable: false,
  requestId: null,
};

export function normalizeAppError(error: unknown): AppErrorEnvelope {
  if (typeof error !== "object" || error === null) return UNKNOWN_ERROR;

  const candidate = error as Partial<AppErrorEnvelope>;
  if (
    typeof candidate.code !== "string" ||
    typeof candidate.message !== "string" ||
    typeof candidate.retryable !== "boolean" ||
    !isAppErrorCategory(candidate.category)
  ) {
    return UNKNOWN_ERROR;
  }

  return {
    code: candidate.code,
    category: candidate.category,
    message: candidate.message,
    retryable: candidate.retryable,
    requestId:
      typeof candidate.requestId === "string" ? candidate.requestId : null,
  };
}
