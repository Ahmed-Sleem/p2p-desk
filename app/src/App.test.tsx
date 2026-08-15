import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type { CoreClient } from "./ipc/client";
import type { BootstrapInfo } from "./ipc/contracts";

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

describe("App", () => {
  it("shows a stable startup state and then trusted core metadata", async () => {
    let resolveInfo: ((value: BootstrapInfo) => void) | undefined;
    const client: CoreClient = {
      getBootstrapInfo: vi.fn(
        () =>
          new Promise<BootstrapInfo>((resolve) => {
            resolveInfo = resolve;
          }),
      ),
    };

    render(<App client={client} />);
    expect(
      screen.getByRole("heading", { name: /opening the trusted local core/i }),
    ).toBeInTheDocument();

    resolveInfo?.(info);
    expect(
      await screen.findByRole("heading", {
        name: /secure local shell connected/i,
      }),
    ).toBeInTheDocument();
    expect(screen.getByText(info.dataRoot)).toBeInTheDocument();
    expect(screen.getByText(/never replaced by demo/i)).toBeInTheDocument();
  });

  it("renders a typed retryable error and retries", async () => {
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

    render(<App client={client} />);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "CORE-STORAGE-PATH",
    );

    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(
      await screen.findByRole("heading", {
        name: /secure local shell connected/i,
      }),
    ).toBeInTheDocument();
    expect(getBootstrapInfo).toHaveBeenCalledTimes(2);
  });

  it("does not expose links or execution actions in the foundation shell", async () => {
    const client: CoreClient = {
      getBootstrapInfo: vi.fn().mockResolvedValue(info),
    };
    render(<App client={client} />);
    await screen.findByRole("heading", {
      name: /secure local shell connected/i,
    });
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
    expect(screen.queryByText(/place order/i)).not.toBeInTheDocument();
  });
});
