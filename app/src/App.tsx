import { useCallback, useEffect, useState } from "react";
import { coreClient, normalizeAppError, type CoreClient } from "./ipc/client";
import type { AppErrorEnvelope, BootstrapInfo } from "./ipc/contracts";
import { lifecycleClient, type LifecycleClient } from "./ipc/lifecycle-client";
import type {
  LifecycleView,
  MarketContextDraft,
  RefreshSettings,
} from "./ipc/lifecycle-contracts";
import { AdvancedFilters } from "./ui/AdvancedFilters";
import { AppShell } from "./ui/AppShell";
import { ContextBar } from "./ui/ContextBar";
import { REFRESH_STAGE_LABELS, type PageId } from "./ui/content";
import { IconButton } from "./ui/primitives";
import { PageContent, type PageActions } from "./ui/PageContent";
import { StateView } from "./ui/StateView";

const SIDEBAR_KEY = "p2p-desk-sidebar";

type StartupState =
  | { readonly kind: "loading" }
  | { readonly kind: "error"; readonly error: AppErrorEnvelope }
  | {
      readonly kind: "ready";
      readonly info: BootstrapInfo;
      readonly view: LifecycleView;
    };

export interface AppProps {
  readonly client?: CoreClient;
  readonly lifecycle?: LifecycleClient;
}

async function loadTrustedState(
  client: CoreClient,
  lifecycle: LifecycleClient,
): Promise<StartupState> {
  try {
    const info = await client.getBootstrapInfo();
    const view = await lifecycle.getView();
    return { kind: "ready", info, view };
  } catch (error: unknown) {
    return { kind: "error", error: normalizeAppError(error) };
  }
}

function initialSidebarState() {
  try {
    return window.localStorage.getItem(SIDEBAR_KEY) === "expanded";
  } catch {
    return false;
  }
}

export function App({
  client = coreClient,
  lifecycle = lifecycleClient,
}: AppProps) {
  const [startup, setStartup] = useState<StartupState>({ kind: "loading" });
  const [page, setPage] = useState<PageId>("overview");
  const [sidebarExpanded, setSidebarExpanded] = useState(initialSidebarState);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [draft, setDraft] = useState<MarketContextDraft | null>(null);
  const [commandError, setCommandError] = useState<AppErrorEnvelope | null>(
    null,
  );
  const [statusMessage, setStatusMessage] = useState("");
  const [localDraftChanged, setLocalDraftChanged] = useState(false);

  const reload = useCallback(async () => {
    setStartup({ kind: "loading" });
    setCommandError(null);
    const next = await loadTrustedState(client, lifecycle);
    if (next.kind === "ready") {
      setDraft(next.view.draft);
      setLocalDraftChanged(false);
    }
    setStartup(next);
  }, [client, lifecycle]);

  useEffect(() => {
    let active = true;
    void loadTrustedState(client, lifecycle).then((next) => {
      if (!active) return;
      if (next.kind === "ready") setDraft(next.view.draft);
      setStartup(next);
    });
    return () => {
      active = false;
    };
  }, [client, lifecycle]);

  useEffect(() => {
    if (startup.kind !== "ready") return;
    let active = true;
    let pending = false;
    const refreshView = async () => {
      if (pending) return;
      pending = true;
      try {
        const view = await lifecycle.getView();
        if (!active) return;
        setStartup((current) =>
          current.kind === "ready" ? { ...current, view } : current,
        );
        if (!localDraftChanged) setDraft(view.draft);
      } catch (error: unknown) {
        if (active) setCommandError(normalizeAppError(error));
      } finally {
        pending = false;
      }
    };
    const timer = window.setInterval(() => void refreshView(), 1_000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [lifecycle, localDraftChanged, startup.kind]);

  useEffect(() => {
    if (startup.kind !== "ready") return;
    const syncConnectivity = () => {
      void lifecycle
        .setOffline(!navigator.onLine)
        .then((view) =>
          setStartup((current) =>
            current.kind === "ready" ? { ...current, view } : current,
          ),
        )
        .catch((error: unknown) => setCommandError(normalizeAppError(error)));
    };
    window.addEventListener("online", syncConnectivity);
    window.addEventListener("offline", syncConnectivity);
    return () => {
      window.removeEventListener("online", syncConnectivity);
      window.removeEventListener("offline", syncConnectivity);
    };
  }, [lifecycle, startup.kind]);

  const execute = useCallback(
    async (
      operation: () => Promise<LifecycleView>,
      successMessage?: string,
    ) => {
      setCommandError(null);
      try {
        const view = await operation();
        setStartup((current) =>
          current.kind === "ready" ? { ...current, view } : current,
        );
        if (successMessage) setStatusMessage(successMessage);
        return view;
      } catch (error: unknown) {
        setCommandError(normalizeAppError(error));
        return null;
      }
    },
    [],
  );

  const toggleSidebar = () =>
    setSidebarExpanded((current) => {
      const next = !current;
      try {
        window.localStorage.setItem(
          SIDEBAR_KEY,
          next ? "expanded" : "collapsed",
        );
      } catch {
        /* persistence is optional */
      }
      return next;
    });
  const changePage = (next: PageId) => {
    setPage(next);
    if (
      sidebarExpanded &&
      typeof window.matchMedia === "function" &&
      window.matchMedia("(max-width: 1180px)").matches
    ) {
      setSidebarExpanded(false);
      try {
        window.localStorage.setItem(SIDEBAR_KEY, "collapsed");
      } catch {
        /* persistence is optional */
      }
    }
    window.requestAnimationFrame(() =>
      document.getElementById("workspace")?.focus(),
    );
  };

  if (startup.kind !== "ready") {
    return (
      <AppShell
        page={page}
        expanded={sidebarExpanded}
        onPageChange={changePage}
        onToggle={toggleSidebar}
      >
        {startup.kind === "loading" ? (
          <StateView
            tone="loading"
            title="Opening the trusted local core"
            detail="Checking application identity, local storage, lifecycle settings, and runtime prerequisites."
            live
          />
        ) : (
          <StateView
            tone="error"
            title="P2P Desk could not initialize safely"
            detail={`${startup.error.message} No demo, cached, historical, or secondary value was substituted.`}
          >
            <button
              className="button button-primary"
              type="button"
              onClick={() => void reload()}
            >
              Retry
            </button>
            <p className="error-code">
              {startup.error.code} · {startup.error.category}
            </p>
          </StateView>
        )}
      </AppShell>
    );
  }

  const view = startup.view;
  const currentDraft = draft ?? view.draft;
  const updateDraft = (next: MarketContextDraft) => {
    setDraft(next);
    setLocalDraftChanged(next !== view.draft);
  };
  const refresh = () => void execute(() => lifecycle.refresh());
  const actions: PageActions = {
    refresh,
    cancel: () =>
      void execute(() => lifecycle.cancelRefresh(), "Cancellation requested"),
    reset: () =>
      void execute(() => lifecycle.resetState(), "Saved context reset").then(
        (next) => {
          if (next) {
            setDraft(next.draft);
            setLocalDraftChanged(false);
          }
        },
      ),
    edit: () => {
      setPage("overview");
      setFiltersOpen(true);
    },
    openHealth: () => setPage("health"),
    copyDiagnostics: () => {
      const safe = JSON.stringify(
        {
          status: view.status,
          freshness: view.freshness,
          offline: view.offline,
          requestId: view.requestId,
          maintenanceWarning: view.maintenanceWarning,
        },
        null,
        2,
      );
      void navigator.clipboard
        ?.writeText(safe)
        .then(() => setStatusMessage("Safe diagnostics copied"))
        .catch(() => setStatusMessage("Clipboard is unavailable"));
    },
    updateSettings: (settings: RefreshSettings) =>
      void execute(
        () => lifecycle.updateSettings(settings),
        "Refresh settings saved",
      ),
  };
  const applying = view.status.kind === "refreshing";
  const lastUpdate =
    view.lastSuccessMs === null
      ? "Never updated"
      : new Intl.DateTimeFormat(undefined, {
          hour: "2-digit",
          minute: "2-digit",
        }).format(new Date(view.lastSuccessMs));
  const refreshLabel =
    view.status.kind === "refreshing"
      ? REFRESH_STAGE_LABELS[view.status.stage]
      : view.secondsUntilRefresh === null
        ? lastUpdate
        : `Refresh in ${view.secondsUntilRefresh}s`;

  return (
    <AppShell
      page={page}
      expanded={sidebarExpanded}
      onPageChange={changePage}
      onToggle={toggleSidebar}
      topbarActions={
        <>
          <span className="last-update" aria-live="polite">
            {refreshLabel}
          </span>
          <IconButton
            icon="refresh"
            label="Refresh the applied context"
            disabled={applying || view.offline}
            onClick={refresh}
          />
        </>
      }
    >
      <ContextBar
        draft={currentDraft}
        unapplied={localDraftChanged || view.unappliedChanges}
        disabled={applying}
        onDraftChange={updateDraft}
        onOpenFilters={() => setFiltersOpen(true)}
        onApply={() =>
          void execute(() => lifecycle.apply(currentDraft)).then((next) => {
            if (next) {
              setDraft(next.draft);
              setLocalDraftChanged(false);
            }
          })
        }
      />
      {commandError ? (
        <div className="command-error" role="alert">
          <div>
            <strong>{commandError.message}</strong>
            <span>
              {commandError.code} · {commandError.category}
            </span>
          </div>
          <button
            type="button"
            onClick={() => setCommandError(null)}
            aria-label="Dismiss error"
          >
            ×
          </button>
        </div>
      ) : null}
      <PageContent
        page={page}
        view={view}
        info={startup.info}
        actions={actions}
      />
      <AdvancedFilters
        open={filtersOpen}
        draft={currentDraft}
        onChange={updateDraft}
        onClose={() => setFiltersOpen(false)}
      />
      <div
        className="live-status visually-hidden"
        role="status"
        aria-live="polite"
      >
        {statusMessage}
      </div>
    </AppShell>
  );
}
