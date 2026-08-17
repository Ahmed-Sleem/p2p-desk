export type AmountMode = "fiat" | "asset";
export type PaymentLogic = "ANY" | "ALL";

export interface RefreshSettings {
  readonly autoRefresh: boolean;
  readonly intervalSeconds: number;
}

export interface MarketContextDraft {
  readonly asset: string;
  readonly fiat: string;
  readonly amount: string;
  readonly amountMode: AmountMode;
  readonly selectedPaymentMethods: readonly string[];
  readonly paymentLogic: PaymentLogic;
  readonly minimumOrders: number;
  readonly minimumCompletionPercent: string;
  readonly minimumPositivePercent: string;
  readonly proOnly: boolean;
  readonly maximumBuyPrice: string | null;
  readonly minimumSellPrice: string | null;
  readonly resultsTarget: number;
}

export type StartupStage =
  "loading-settings" | "restoring-context" | "loading-catalog" | "ready";

export type RefreshTrigger =
  "apply" | "manual" | "automatic" | "startup" | "wake";

export type RefreshStage =
  | "queued"
  | "acquiring"
  | "validating"
  | "calculating"
  | "committing"
  | "maintaining";

export type EmptyKind = "provider-empty" | "no-matching-results";

export type FailureKind =
  | "invalid-restored-state"
  | "offline"
  | "provider"
  | "validation"
  | "calculation"
  | "persistence"
  | "cancelled"
  | "busy";

export interface ActionableFailure {
  readonly kind: FailureKind;
  readonly title: string;
  readonly detail: string;
  readonly retryable: boolean;
  readonly action: string;
}

export type LifecycleStatus =
  | { readonly kind: "loading"; readonly stage: StartupStage }
  | { readonly kind: "ready"; readonly lastSuccessMs: number | null }
  | {
      readonly kind: "refreshing";
      readonly requestId: string;
      readonly trigger: RefreshTrigger;
      readonly stage: RefreshStage;
      readonly previousValuesHidden: true;
    }
  | {
      readonly kind: "empty";
      readonly emptyKind: EmptyKind;
      readonly detail: string;
      readonly retryable: boolean;
    }
  | { readonly kind: "error"; readonly failure: ActionableFailure };

export type Freshness = "never-loaded" | "fresh" | "stale" | "clock-anomaly";

export interface LifecycleView {
  readonly status: LifecycleStatus;
  readonly settings: RefreshSettings;
  readonly draft: MarketContextDraft;
  readonly applied: MarketContextDraft;
  readonly unappliedChanges: boolean;
  readonly lastSuccessMs: number | null;
  readonly nextRefreshDueMs: number | null;
  readonly secondsUntilRefresh: number | null;
  readonly freshness: Freshness;
  readonly maintenanceWarning: string | null;
  readonly requestId: string | null;
  readonly offline: boolean;
}

export type ProviderProgressStage =
  | "queued"
  | "waiting-for-rate-limit"
  | "requesting"
  | "backing-off"
  | "validating"
  | "complete";

export interface SideProgress {
  readonly nextPage: number;
  readonly fetched: number;
  readonly valid: number;
  readonly duplicates: number;
  readonly rejected: number;
  readonly target: number;
  readonly providerTotal: number | null;
  readonly exhausted: boolean;
}

export interface AcquisitionProgress {
  readonly stage: ProviderProgressStage;
  readonly activeIntent: "buy-asset" | "sell-asset" | null;
  readonly attemptsForPage: number;
  readonly requestsCompleted: number;
  readonly buy: SideProgress;
  readonly sell: SideProgress;
}
