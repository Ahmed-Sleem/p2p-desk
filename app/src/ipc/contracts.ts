export const PRODUCT_NAME = "P2P Desk" as const;
export const PRODUCT_SUBTITLE = "Read-only P2P decision terminal" as const;

export type HostPlatform = "windows" | "linux" | "macos" | "unknown";
export type PrerequisiteStatus = "available" | "not-applicable" | "missing";

export interface BuildInfo {
  readonly appVersion: string;
  readonly buildProfile: "debug" | "release";
  readonly schemaVersion: number;
  readonly calculationVersion: number;
  readonly providerAdapterVersion: number;
}

export interface WindowPolicy {
  readonly firstWidth: number;
  readonly firstHeight: number;
  readonly minimumWidth: number;
  readonly minimumHeight: number;
  readonly restoresSafeNormalBounds: true;
  readonly nativeDecorations: true;
}

export interface RuntimePrerequisite {
  readonly name: "Microsoft Edge WebView2" | "Platform system webview";
  readonly mode: "system";
  readonly status: PrerequisiteStatus;
  readonly version: string | null;
  readonly remediation: string | null;
}

export interface BootstrapInfo {
  readonly productName: typeof PRODUCT_NAME;
  readonly subtitle: typeof PRODUCT_SUBTITLE;
  readonly hostPlatform: HostPlatform;
  readonly dataRoot: string;
  readonly build: BuildInfo;
  readonly windowPolicy: WindowPolicy;
  readonly runtime: RuntimePrerequisite;
}

export type AppErrorCategory =
  | "configuration"
  | "prerequisite"
  | "storage"
  | "provider"
  | "lifecycle"
  | "internal";

export interface AppErrorEnvelope {
  readonly code: string;
  readonly category: AppErrorCategory;
  readonly message: string;
  readonly retryable: boolean;
  readonly requestId: string | null;
}
