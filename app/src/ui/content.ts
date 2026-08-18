import type {
  FailureKind,
  RefreshStage,
  StartupStage,
} from "../ipc/lifecycle-contracts";

export const PAGE_IDS = [
  "overview",
  "offers",
  "analysis",
  "history",
  "health",
  "settings",
] as const;

export type PageId = (typeof PAGE_IDS)[number];

export interface NavigationItem {
  readonly id: PageId;
  readonly label: string;
  readonly icon:
    "overview" | "offers" | "analysis" | "history" | "health" | "settings";
}

export const NAVIGATION: readonly NavigationItem[] = [
  { id: "overview", label: "Overview", icon: "overview" },
  { id: "offers", label: "Offers", icon: "offers" },
  { id: "analysis", label: "Analysis", icon: "analysis" },
  { id: "history", label: "History", icon: "history" },
  { id: "health", label: "Data Health", icon: "health" },
  { id: "settings", label: "Settings", icon: "settings" },
];

export const STARTUP_STAGE_LABELS: Readonly<Record<StartupStage, string>> = {
  "loading-settings": "Loading validated settings",
  "restoring-context": "Restoring the saved decision context",
  "loading-catalog": "Loading the validated pair catalog",
  ready: "Preparing the workspace",
};

export const REFRESH_STAGE_LABELS: Readonly<Record<RefreshStage, string>> = {
  queued: "Queueing the two-sided refresh",
  acquiring: "Acquiring Buy and Sell pages",
  validating: "Validating provider rows and side meaning",
  calculating: "Running exact amount-aware calculations",
  committing: "Publishing the complete two-sided snapshot",
  maintaining: "Completing local retention maintenance",
};

export const FAILURE_IMPACT: Readonly<Record<FailureKind, string>> = {
  "invalid-restored-state":
    "Saved settings are blocked and no live context was substituted.",
  offline: "Live panels are hidden while the provider cannot be reached.",
  provider: "No partial provider response was published as live data.",
  validation: "The response failed closed before any live publication.",
  calculation:
    "No value was published because exact calculation did not complete.",
  persistence:
    "Validated data was not labeled live because atomic storage did not complete.",
  cancelled:
    "The operation stopped without publishing partial or previous values.",
  busy: "The existing refresh continues; no overlapping request was started.",
};

export const PAGE_EMPTY_COPY: Readonly<
  Record<
    Exclude<PageId, "overview" | "settings" | "health">,
    { readonly title: string; readonly detail: string }
  >
> = {
  offers: {
    title: "No current offer table to show",
    detail:
      "Offers appear only from a complete validated two-sided live snapshot. Previous, historical, and partial rows are never substituted.",
  },
  analysis: {
    title: "Analysis needs a complete live snapshot",
    detail:
      "Charts and metrics remain withheld until both sides, exact calculations, and atomic publication succeed.",
  },
  history: {
    title: "No historical view is selected",
    detail:
      "Timestamped history stays visibly historical and never replaces a failed live acquisition.",
  },
};
