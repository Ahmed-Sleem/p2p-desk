import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type { CoreClient } from "./ipc/client";
import type { BootstrapInfo } from "./ipc/contracts";
import type { LifecycleClient } from "./ipc/lifecycle-client";
import type { LifecycleView } from "./ipc/lifecycle-contracts";

const info: BootstrapInfo = {
  productName: "P2P Desk",
  subtitle: "Read-only P2P decision terminal",
  hostPlatform: "windows",
  dataRoot: "C:\\Users\\Test\\AppData\\Local\\P2P Desk",
  build: {
    appVersion: "0.1.0",
    buildProfile: "debug",
    schemaVersion: 1,
    calculationVersion: 1,
    providerAdapterVersion: 1,
  },
  windowPolicy: {
    firstWidth: 1280,
    firstHeight: 800,
    minimumWidth: 1024,
    minimumHeight: 700,
    restoresSafeNormalBounds: true,
    nativeDecorations: true,
  },
  runtime: {
    name: "Microsoft Edge WebView2",
    mode: "system",
    status: "available",
    version: "140.0.0.0",
    remediation: null,
  },
};

const baseView: LifecycleView = {
  status: { kind: "ready", lastSuccessMs: null },
  settings: { autoRefresh: true, intervalSeconds: 20 },
  draft: {
    asset: "USDT",
    fiat: "EGP",
    amount: "10000",
    amountMode: "fiat",
    selectedPaymentMethods: [],
    paymentLogic: "ANY",
    minimumOrders: 0,
    minimumCompletionPercent: "0",
    minimumPositivePercent: "0",
    proOnly: false,
    maximumBuyPrice: null,
    minimumSellPrice: null,
    resultsTarget: 40,
  },
  applied: {
    asset: "USDT",
    fiat: "EGP",
    amount: "10000",
    amountMode: "fiat",
    selectedPaymentMethods: [],
    paymentLogic: "ANY",
    minimumOrders: 0,
    minimumCompletionPercent: "0",
    minimumPositivePercent: "0",
    proOnly: false,
    maximumBuyPrice: null,
    minimumSellPrice: null,
    resultsTarget: 40,
  },
  unappliedChanges: false,
  lastSuccessMs: null,
  nextRefreshDueMs: null,
  secondsUntilRefresh: null,
  freshness: "never-loaded",
  maintenanceWarning: null,
  requestId: null,
  offline: false,
};

function lifecycleMock(view: LifecycleView = baseView): LifecycleClient {
  return {
    getView: vi.fn().mockResolvedValue(view),
    resetState: vi.fn().mockResolvedValue(view),
    updateDraft: vi.fn().mockResolvedValue(view),
    updateSettings: vi.fn().mockResolvedValue(view),
    apply: vi.fn().mockResolvedValue(view),
    refresh: vi.fn().mockResolvedValue(view),
    refreshIfDue: vi.fn().mockResolvedValue(view),
    refreshAfterWake: vi.fn().mockResolvedValue(view),
    setOffline: vi.fn().mockResolvedValue(view),
    cancelRefresh: vi.fn().mockResolvedValue(view),
  };
}

function renderReady(view: LifecycleView = baseView) {
  const client: CoreClient = {
    getBootstrapInfo: vi.fn().mockResolvedValue(info),
  };
  const lifecycle = lifecycleMock(view);
  render(<App client={client} lifecycle={lifecycle} />);
  return { client, lifecycle };
}

describe("approved application shell", () => {
  it("shows an immediate stable startup state and then the trusted shared context", async () => {
    let resolveInfo: ((value: BootstrapInfo) => void) | undefined;
    const client: CoreClient = {
      getBootstrapInfo: vi.fn(
        () =>
          new Promise<BootstrapInfo>((resolve) => {
            resolveInfo = resolve;
          }),
      ),
    };
    const lifecycle = lifecycleMock();
    render(<App client={client} lifecycle={lifecycle} />);
    expect(
      screen.getByRole("heading", { name: /opening the trusted local core/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("navigation", { name: /primary/i }),
    ).toBeInTheDocument();
    resolveInfo?.(info);
    expect(
      await screen.findByRole("heading", {
        name: /ready for the first validated refresh/i,
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("region", { name: /shared decision context/i }),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(/experimental source · no fallback/i),
    ).not.toBeInTheDocument();
  });

  it("renders a typed startup error and retries without fallback", async () => {
    const getBootstrapInfo = vi
      .fn<CoreClient["getBootstrapInfo"]>()
      .mockRejectedValueOnce({
        code: "CORE-STORAGE-PATH",
        category: "storage",
        message: "The local data path is unavailable.",
        retryable: true,
        requestId: null,
      })
      .mockResolvedValueOnce(info);
    const client: CoreClient = { getBootstrapInfo };
    const lifecycle = lifecycleMock();
    render(<App client={client} lifecycle={lifecycle} />);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "No demo, cached, historical, or secondary value was substituted",
    );
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(
      await screen.findByRole("heading", {
        name: /ready for the first validated refresh/i,
      }),
    ).toBeInTheDocument();
    expect(getBootstrapInfo).toHaveBeenCalledTimes(2);
  });

  it("navigates all six pages and keeps current-page semantics", async () => {
    renderReady();
    await screen.findByRole("heading", {
      name: /ready for the first validated refresh/i,
    });
    const navigation = screen.getByRole("navigation", { name: /primary/i });
    await userEvent.click(
      within(navigation).getByRole("button", { name: "Data Health" }),
    );
    expect(
      screen.getByRole("heading", { name: "Data Health", level: 1 }),
    ).toBeInTheDocument();
    expect(
      within(navigation).getByRole("button", { name: "Data Health" }),
    ).toHaveAttribute("aria-current", "page");
    expect(screen.getByText(/no fallback or merge/i)).toBeInTheDocument();
  });

  it("marks draft changes as unapplied and sends the exact draft to Apply", async () => {
    const { lifecycle } = renderReady();
    const apply = vi.spyOn(lifecycle, "apply");
    const amount = await screen.findByRole("textbox", {
      name: "Transaction amount",
    });
    await userEvent.clear(amount);
    await userEvent.type(amount, "12000.50");
    expect(screen.getByText("Unapplied changes")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Apply" }));
    expect(apply).toHaveBeenCalledWith(
      expect.objectContaining({ amount: "12000.50" }),
    );
  });

  it("shows a localized refreshing boundary and cancellation action", async () => {
    const refreshing: LifecycleView = {
      ...baseView,
      status: {
        kind: "refreshing",
        requestId: "request-safe",
        trigger: "manual",
        stage: "acquiring",
        previousValuesHidden: true,
      },
      requestId: "request-safe",
    };
    const { lifecycle } = renderReady(refreshing);
    const cancelRefresh = vi.spyOn(lifecycle, "cancelRefresh");
    expect(
      await screen.findByRole("heading", {
        name: /acquiring buy and sell pages/i,
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/previous live values are hidden/i),
    ).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "Cancel refresh" }),
    );
    expect(cancelRefresh).toHaveBeenCalledTimes(1);
  });

  it("toggles the accessible sidebar state without changing the current page", async () => {
    renderReady();
    await screen.findByRole("heading", {
      name: /ready for the first validated refresh/i,
    });
    const expand = screen.getByRole("button", { name: "Expand sidebar" });
    await userEvent.click(expand);
    expect(
      screen.getByRole("button", { name: "Collapse sidebar" }),
    ).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("button", { name: "Overview" })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });

  it("opens keyboard-accessible advanced context controls", async () => {
    renderReady();
    await screen.findByRole("heading", {
      name: /ready for the first validated refresh/i,
    });
    await userEvent.click(screen.getByRole("button", { name: "More filters" }));
    const dialog = screen.getByRole("dialog", {
      name: /market and eligibility filters/i,
    });
    await userEvent.click(within(dialog).getByRole("button", { name: "ALL" }));
    expect(within(dialog).getByRole("button", { name: "ALL" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(
      within(dialog).getByRole("spinbutton", { name: "Results per side" }),
    ).toHaveValue(40);
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Close filters" }),
    );
    expect(
      screen.queryByRole("dialog", { name: /market and eligibility filters/i }),
    ).not.toBeInTheDocument();
  });

  it("renders invalid restored state with explicit reset and no implicit defaults", async () => {
    const invalid: LifecycleView = {
      ...baseView,
      status: {
        kind: "error",
        failure: {
          kind: "invalid-restored-state",
          title: "Saved settings could not be restored",
          detail: "Saved lifecycle JSON is invalid.",
          retryable: false,
          action: "Reset saved settings explicitly",
        },
      },
    };
    const { lifecycle } = renderReady(invalid);
    const reset = vi.spyOn(lifecycle, "resetState");
    expect(await screen.findByRole("alert")).toHaveTextContent(
      /no live context was substituted/i,
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Reset saved context" }),
    );
    expect(reset).toHaveBeenCalledTimes(1);
  });

  it("places source disclosure in Settings rather than global chrome", async () => {
    renderReady();
    await screen.findByRole("heading", {
      name: /ready for the first validated refresh/i,
    });
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(
      screen.getByRole("heading", { name: "Experimental source" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/binance p2p web can change without notice/i),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("switch", { name: "Auto-refresh" }),
    ).toHaveAttribute("aria-checked", "true");
  });

  it("keeps the read-only boundary and does not expose navigation links or execution actions", async () => {
    renderReady();
    await screen.findByRole("heading", {
      name: /ready for the first validated refresh/i,
    });
    expect(
      screen.queryByRole("link", { name: /provider|order/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(/place order|execute order|open on binance/i),
    ).not.toBeInTheDocument();
    const metricTitleIds = Array.from(
      document.querySelectorAll<HTMLDialogElement>(
        ".metric-foundations dialog[aria-labelledby]",
      ),
      (dialog) => dialog.getAttribute("aria-labelledby"),
    );
    expect(metricTitleIds).toHaveLength(2);
    expect(new Set(metricTitleIds).size).toBe(2);
    await userEvent.click(
      screen.getByRole("button", { name: /explain best eligible buy price/i }),
    );
    expect(
      within(screen.getByRole("dialog")).getByText(
        /never prepares, hands off, or executes/i,
      ),
    ).toBeInTheDocument();
  });
});
