import type { BootstrapInfo } from "../ipc/contracts";
import type {
  LifecycleView,
  RefreshSettings,
} from "../ipc/lifecycle-contracts";
import {
  FAILURE_IMPACT,
  PAGE_EMPTY_COPY,
  REFRESH_STAGE_LABELS,
  STARTUP_STAGE_LABELS,
  type PageId,
} from "./content";
import { Button, Chip, Panel } from "./primitives";
import { StateView } from "./StateView";
import { MetricHelp } from "./MetricHelp";

export interface PageActions {
  readonly refresh: () => void;
  readonly cancel: () => void;
  readonly reset: () => void;
  readonly edit: () => void;
  readonly openHealth: () => void;
  readonly copyDiagnostics: () => void;
  readonly updateSettings: (settings: RefreshSettings) => void;
}

function formatObservation(timestamp: number | null) {
  if (timestamp === null) return "Never";
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "medium",
  }).format(new Date(timestamp));
}

function LifecycleBoundary({
  view,
  actions,
}: {
  readonly view: LifecycleView;
  readonly actions: PageActions;
}) {
  const status = view.status;
  if (status.kind === "loading")
    return (
      <StateView
        tone="loading"
        title={STARTUP_STAGE_LABELS[status.stage]}
        detail="The shell stays available while trusted local state is restored. No default or previous live value is substituted."
        live
      />
    );
  if (status.kind === "refreshing")
    return (
      <StateView
        tone="loading"
        title={REFRESH_STAGE_LABELS[status.stage]}
        detail="Previous live values are hidden until Buy and Sell both validate and commit atomically."
        live
      >
        <Button onClick={actions.cancel}>Cancel refresh</Button>
      </StateView>
    );
  if (status.kind === "error")
    return (
      <StateView
        tone={status.failure.kind === "offline" ? "offline" : "error"}
        title={status.failure.title}
        detail={`${status.failure.detail} ${FAILURE_IMPACT[status.failure.kind]}`}
      >
        <Button
          variant="primary"
          onClick={actions.refresh}
          disabled={!status.failure.retryable}
        >
          Retry
        </Button>
        <Button icon="edit" onClick={actions.edit}>
          Edit context
        </Button>
        <Button onClick={actions.openHealth}>Data Health</Button>
        <Button icon="copy" onClick={actions.copyDiagnostics}>
          Copy diagnostics
        </Button>
        {status.failure.kind === "invalid-restored-state" ? (
          <Button variant="danger" onClick={actions.reset}>
            Reset saved context
          </Button>
        ) : null}
      </StateView>
    );
  if (status.kind === "empty")
    return (
      <StateView
        tone="empty"
        title={
          status.emptyKind === "provider-empty"
            ? "The provider confirmed an empty market"
            : "No results match the applied context"
        }
        detail={`${status.detail} No historical or fabricated row is shown as live.`}
      >
        <Button variant="primary" onClick={actions.refresh}>
          Refresh again
        </Button>
        <Button icon="edit" onClick={actions.edit}>
          Edit context
        </Button>
      </StateView>
    );
  if (view.lastSuccessMs === null)
    return (
      <StateView
        tone="empty"
        title="Ready for the first validated refresh"
        detail="The shared context is valid. Start a live request to acquire, validate, calculate, and atomically publish both sides."
      >
        <Button variant="primary" icon="refresh" onClick={actions.refresh}>
          Refresh now
        </Button>
      </StateView>
    );
  return (
    <StateView
      tone="success"
      title="Complete two-sided snapshot committed"
      detail={`The lifecycle is ready and the latest validated commit is ${formatObservation(view.lastSuccessMs)}. Result values remain hidden unless their complete typed snapshot is available.`}
    >
      <Button variant="primary" icon="refresh" onClick={actions.refresh}>
        Refresh now
      </Button>
    </StateView>
  );
}

function SettingsPage({
  view,
  info,
  actions,
}: {
  readonly view: LifecycleView;
  readonly info: BootstrapInfo;
  readonly actions: PageActions;
}) {
  return (
    <div className="page-grid settings-layout">
      <Panel
        title="Refresh controls"
        description="Validated local lifecycle settings."
        action={<Chip tone="success">Saved locally</Chip>}
      >
        <div className="settings-list">
          <div className="setting-row">
            <div>
              <strong>Auto-refresh</strong>
              <p>
                Runs complete two-sided refreshes while the app process is open.
              </p>
            </div>
            <button
              className="switch"
              type="button"
              role="switch"
              aria-checked={view.settings.autoRefresh}
              aria-label="Auto-refresh"
              onClick={() =>
                actions.updateSettings({
                  ...view.settings,
                  autoRefresh: !view.settings.autoRefresh,
                })
              }
            />
          </div>
          <div className="setting-row">
            <div>
              <strong>Refresh interval</strong>
              <p>
                Whole seconds from 10 through 3,600; countdown restarts after
                success.
              </p>
            </div>
            <label className="number-control">
              <input
                aria-label="Refresh interval"
                type="number"
                min="10"
                max="3600"
                step="1"
                value={view.settings.intervalSeconds}
                onChange={(event) =>
                  actions.updateSettings({
                    ...view.settings,
                    intervalSeconds: Number(event.target.value),
                  })
                }
              />
              <span>seconds</span>
            </label>
          </div>
        </div>
      </Panel>
      <Panel
        title="Experimental source"
        description="Risk disclosure belongs here and in Data Health."
        action={<Chip tone="warning">Unsupported contract</Chip>}
      >
        <div className="disclosure">
          <strong>Binance P2P Web can change without notice.</strong>
          <p>
            The website contract may rate-limit, block, change schema, or become
            unavailable. Cached, historical, Agent, secondary, and fabricated
            values are never presented as current live data.
          </p>
        </div>
      </Panel>
      <Panel
        title="About this build"
        description="Trusted local application metadata."
      >
        <dl className="definition-list">
          <div>
            <dt>Application</dt>
            <dd>
              {info.productName} {info.build.appVersion}
            </dd>
          </div>
          <div>
            <dt>Platform</dt>
            <dd>{info.hostPlatform}</dd>
          </div>
          <div>
            <dt>System webview</dt>
            <dd>{info.runtime.status}</dd>
          </div>
          <div>
            <dt>Data location</dt>
            <dd className="break-value">{info.dataRoot}</dd>
          </div>
          <div>
            <dt>Read-only boundary</dt>
            <dd>No credentials, account, handoff, preparation, or execution</dd>
          </div>
        </dl>
      </Panel>
    </div>
  );
}

function HealthPage({ view }: { readonly view: LifecycleView }) {
  const statusLabel =
    view.status.kind === "ready"
      ? "Ready"
      : view.status.kind === "refreshing"
        ? "Refreshing"
        : view.status.kind === "error"
          ? "Blocked"
          : view.status.kind === "empty"
            ? "Empty"
            : "Starting";
  return (
    <div className="page-grid">
      <Panel
        title="Lifecycle health"
        description="Current trusted state; no provider identity or raw response."
        action={
          <Chip tone={view.status.kind === "ready" ? "success" : "warning"}>
            {statusLabel}
          </Chip>
        }
      >
        <dl className="definition-list">
          <div>
            <dt>Freshness</dt>
            <dd>{view.freshness}</dd>
          </div>
          <div>
            <dt>Offline</dt>
            <dd>{view.offline ? "Yes" : "No"}</dd>
          </div>
          <div>
            <dt>Last complete commit</dt>
            <dd>{formatObservation(view.lastSuccessMs)}</dd>
          </div>
          <div>
            <dt>Request ID</dt>
            <dd className="break-value">{view.requestId ?? "None"}</dd>
          </div>
          <div>
            <dt>Maintenance</dt>
            <dd>{view.maintenanceWarning ?? "No warning"}</dd>
          </div>
        </dl>
      </Panel>
      <Panel
        title="Source contract"
        description="Experimental Binance P2P Web — ads source only."
        action={<Chip tone="warning">Fail closed</Chip>}
      >
        <div className="disclosure">
          <strong>No fallback or merge.</strong>
          <p>
            Pair, side, schema, exact decimal, payment, amount, and eligibility
            checks must pass for both sides before atomic publication. Agent
            endpoints remain separate metadata and health only.
          </p>
        </div>
      </Panel>
    </div>
  );
}

export function PageContent({
  page,
  view,
  info,
  actions,
}: {
  readonly page: PageId;
  readonly view: LifecycleView;
  readonly info: BootstrapInfo;
  readonly actions: PageActions;
}) {
  if (page === "overview")
    return (
      <div className="page-grid">
        <LifecycleBoundary view={view} actions={actions} />
        <div
          className="metric-foundations"
          aria-label="Metric explanation foundations"
        >
          <Panel
            title="Buy asset"
            description="Advertiser sells · equal two-sided prominence"
            action={
              <MetricHelp
                entry={{
                  title: "Best eligible Buy price",
                  meaning:
                    "The lowest validated full-amount offer for buying the asset.",
                  calculation:
                    "Amount, limits, availability, payment, merchant, side, and pair checks precede deterministic price ranking.",
                  exclusions:
                    "Partial, stale, invalid, historical, Agent, cached, and demo data.",
                }}
              />
            }
          >
            <p className="withheld-value">
              — <small>withheld until a typed live result is available</small>
            </p>
          </Panel>
          <Panel
            title="Sell asset"
            description="Advertiser buys · equal two-sided prominence"
            action={
              <MetricHelp
                entry={{
                  title: "Best eligible Sell price",
                  meaning:
                    "The highest validated full-amount offer for selling the asset.",
                  calculation:
                    "The same shared amount and filters are applied with the inverse provider-side invariant.",
                  exclusions:
                    "Partial, stale, invalid, historical, Agent, cached, and demo data.",
                }}
              />
            }
          >
            <p className="withheld-value">
              — <small>withheld until a typed live result is available</small>
            </p>
          </Panel>
        </div>
      </div>
    );
  if (page === "settings")
    return <SettingsPage view={view} info={info} actions={actions} />;
  if (page === "health") return <HealthPage view={view} />;
  const copy = PAGE_EMPTY_COPY[page];
  return (
    <StateView tone="empty" title={copy.title} detail={copy.detail}>
      <Button variant="primary" icon="refresh" onClick={actions.refresh}>
        Refresh live context
      </Button>
    </StateView>
  );
}
