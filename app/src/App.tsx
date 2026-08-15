import { useCallback, useEffect, useState } from "react";
import { coreClient, normalizeAppError, type CoreClient } from "./ipc/client";
import {
  PRODUCT_NAME,
  PRODUCT_SUBTITLE,
  type AppErrorEnvelope,
  type BootstrapInfo,
} from "./ipc/contracts";

type BootstrapState =
  | { readonly kind: "loading" }
  | { readonly kind: "ready"; readonly info: BootstrapInfo }
  | { readonly kind: "error"; readonly error: AppErrorEnvelope };

const navigation = [
  "Overview",
  "Offers",
  "Analysis",
  "History",
  "Data Health",
  "Settings",
] as const;

export interface AppProps {
  readonly client?: CoreClient;
}

async function requestBootstrap(client: CoreClient): Promise<BootstrapState> {
  try {
    const info = await client.getBootstrapInfo();
    return { kind: "ready", info };
  } catch (error: unknown) {
    return { kind: "error", error: normalizeAppError(error) };
  }
}

export function App({ client = coreClient }: AppProps) {
  const [state, setState] = useState<BootstrapState>({ kind: "loading" });

  const retry = useCallback(async () => {
    setState({ kind: "loading" });
    setState(await requestBootstrap(client));
  }, [client]);

  useEffect(() => {
    let active = true;
    void requestBootstrap(client).then((nextState) => {
      if (active) setState(nextState);
    });
    return () => {
      active = false;
    };
  }, [client]);

  return (
    <div className="app-shell">
      <aside className="sidebar" aria-label="Primary">
        <div className="brand-mark" aria-hidden="true">
          P2P
        </div>
        <nav>
          <ul className="nav-list">
            {navigation.map((item, index) => (
              <li key={item}>
                <button
                  className={
                    index === 0 ? "nav-item nav-item-active" : "nav-item"
                  }
                  type="button"
                  aria-current={index === 0 ? "page" : undefined}
                >
                  {item}
                </button>
              </li>
            ))}
          </ul>
        </nav>
      </aside>

      <div className="workspace">
        <header className="topbar">
          <div>
            <p className="eyebrow">Local decision terminal</p>
            <h1>{PRODUCT_NAME}</h1>
            <p className="subtitle">{PRODUCT_SUBTITLE}</p>
          </div>
          <div className="source-badge">Experimental source · no fallback</div>
        </header>

        <main className="main-content">
          {state.kind === "loading" ? <LoadingPanel /> : null}
          {state.kind === "ready" ? <ReadyPanel info={state.info} /> : null}
          {state.kind === "error" ? (
            <ErrorPanel error={state.error} onRetry={retry} />
          ) : null}
        </main>
      </div>
    </div>
  );
}

function LoadingPanel() {
  return (
    <section className="status-card" aria-busy="true" aria-live="polite">
      <div className="status-icon status-icon-loading" aria-hidden="true" />
      <p className="eyebrow">Starting app</p>
      <h2>Opening the trusted local core</h2>
      <p>
        Checking application paths, build identity, and runtime prerequisites.
      </p>
    </section>
  );
}

function ReadyPanel({ info }: { readonly info: BootstrapInfo }) {
  return (
    <section className="status-card" aria-labelledby="foundation-heading">
      <div className="status-icon status-icon-ready" aria-hidden="true">
        ✓
      </div>
      <p className="eyebrow">Foundation ready</p>
      <h2 id="foundation-heading">Secure local shell connected</h2>
      <p>
        P2P Desk is using its trusted core. Live market results are never
        replaced by demo, cached, historical, or secondary-source values.
      </p>
      <dl className="detail-grid">
        <div>
          <dt>Application version</dt>
          <dd>{info.build.appVersion}</dd>
        </div>
        <div>
          <dt>Host platform</dt>
          <dd>{info.hostPlatform}</dd>
        </div>
        <div>
          <dt>Runtime</dt>
          <dd>{info.runtime.status}</dd>
        </div>
        <div>
          <dt>Data location</dt>
          <dd className="path-value">{info.dataRoot}</dd>
        </div>
      </dl>
    </section>
  );
}

function ErrorPanel({
  error,
  onRetry,
}: {
  readonly error: AppErrorEnvelope;
  readonly onRetry: () => Promise<void>;
}) {
  return (
    <section className="status-card status-card-error" role="alert">
      <div className="status-icon status-icon-error" aria-hidden="true">
        !
      </div>
      <p className="eyebrow">Startup blocked</p>
      <h2>P2P Desk could not initialize safely</h2>
      <p>{error.message}</p>
      <p className="error-code">
        {error.code} · {error.category}
      </p>
      {error.retryable ? (
        <button
          className="primary-button"
          type="button"
          onClick={() => void onRetry()}
        >
          Retry
        </button>
      ) : null}
    </section>
  );
}
