import { invoke } from "@tauri-apps/api/core";
import type {
  LifecycleView,
  MarketContextDraft,
  RefreshSettings,
} from "./lifecycle-contracts";

export interface LifecycleClient {
  getView(): Promise<LifecycleView>;
  resetState(): Promise<LifecycleView>;
  updateDraft(draft: MarketContextDraft): Promise<LifecycleView>;
  updateSettings(settings: RefreshSettings): Promise<LifecycleView>;
  apply(draft: MarketContextDraft): Promise<LifecycleView>;
  refresh(): Promise<LifecycleView>;
  refreshIfDue(): Promise<LifecycleView>;
  refreshAfterWake(): Promise<LifecycleView>;
  setOffline(offline: boolean): Promise<LifecycleView>;
  cancelRefresh(): Promise<LifecycleView>;
}

export const lifecycleClient: LifecycleClient = {
  getView: () => invoke<LifecycleView>("get_lifecycle_view"),
  resetState: () => invoke<LifecycleView>("reset_lifecycle_state"),
  updateDraft: (draft) =>
    invoke<LifecycleView>("update_market_draft", { draft }),
  updateSettings: (settings) =>
    invoke<LifecycleView>("update_refresh_settings", { settings }),
  apply: (draft) => invoke<LifecycleView>("apply_market_context", { draft }),
  refresh: () => invoke<LifecycleView>("refresh_market"),
  refreshIfDue: () => invoke<LifecycleView>("refresh_if_due"),
  refreshAfterWake: () => invoke<LifecycleView>("refresh_after_wake"),
  setOffline: (offline) => invoke<LifecycleView>("set_offline", { offline }),
  cancelRefresh: () => invoke<LifecycleView>("cancel_refresh"),
};
