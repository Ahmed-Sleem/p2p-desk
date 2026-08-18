import { useEffect, useRef, useState } from "react";
import type { MarketContextDraft } from "../ipc/lifecycle-contracts";
import { Button, IconButton } from "./primitives";

export function AdvancedFilters({
  open,
  draft,
  onChange,
  onClose,
}: {
  readonly open: boolean;
  readonly draft: MarketContextDraft;
  readonly onChange: (draft: MarketContextDraft) => void;
  readonly onClose: () => void;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [paymentInput, setPaymentInput] = useState("");
  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);
  const update = <Key extends keyof MarketContextDraft>(
    key: Key,
    value: MarketContextDraft[Key],
  ) => onChange({ ...draft, [key]: value });
  const addPayment = () => {
    const value = paymentInput.trim();
    if (!value || draft.selectedPaymentMethods.includes(value)) return;
    update("selectedPaymentMethods", [...draft.selectedPaymentMethods, value]);
    setPaymentInput("");
  };
  return (
    <dialog
      ref={dialogRef}
      aria-labelledby="filters-title"
      onClose={onClose}
      onClick={(event) => {
        if (event.target === event.currentTarget) event.currentTarget.close();
      }}
    >
      <div className="dialog-head">
        <div>
          <span className="dialog-kicker">Shared context</span>
          <h2 id="filters-title">Market and eligibility filters</h2>
        </div>
        <IconButton
          icon="close"
          label="Close filters"
          onClick={() => dialogRef.current?.close()}
        />
      </div>
      <div className="dialog-body filter-dialog-grid">
        <fieldset>
          <legend>Market pair</legend>
          <div className="dialog-field-grid">
            <label>
              Asset
              <input
                value={draft.asset}
                autoCapitalize="characters"
                onChange={(event) =>
                  update("asset", event.target.value.toUpperCase())
                }
              />
            </label>
            <label>
              Fiat
              <input
                value={draft.fiat}
                autoCapitalize="characters"
                onChange={(event) =>
                  update("fiat", event.target.value.toUpperCase())
                }
              />
            </label>
          </div>
          <p className="field-help">
            Apply runs the trusted live pair and side validation. Invalid pairs
            fail explicitly.
          </p>
        </fieldset>
        <fieldset>
          <legend>Payment context</legend>
          <div
            className="segmented"
            role="group"
            aria-label="Payment matching logic"
          >
            <button
              type="button"
              aria-pressed={draft.paymentLogic === "ANY"}
              onClick={() => update("paymentLogic", "ANY")}
            >
              ANY
            </button>
            <button
              type="button"
              aria-pressed={draft.paymentLogic === "ALL"}
              onClick={() => update("paymentLogic", "ALL")}
            >
              ALL
            </button>
          </div>
          <div className="payment-editor">
            <label>
              Payment method identifier
              <input
                value={paymentInput}
                onChange={(event) => setPaymentInput(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    addPayment();
                  }
                }}
              />
            </label>
            <Button onClick={addPayment}>Add</Button>
          </div>
          <div className="tag-list">
            {draft.selectedPaymentMethods.length === 0 ? (
              <span className="field-help">
                No selection: accept all locally eligible methods.
              </span>
            ) : (
              draft.selectedPaymentMethods.map((method) => (
                <button
                  type="button"
                  key={method}
                  onClick={() =>
                    update(
                      "selectedPaymentMethods",
                      draft.selectedPaymentMethods.filter(
                        (item) => item !== method,
                      ),
                    )
                  }
                >
                  {method}
                  <span aria-hidden="true"> ×</span>
                  <span className="visually-hidden"> remove</span>
                </button>
              ))
            )}
          </div>
        </fieldset>
        <fieldset>
          <legend>Merchant thresholds</legend>
          <div className="dialog-field-grid three">
            <label>
              Minimum orders
              <input
                type="number"
                min="0"
                max="1000000"
                step="1"
                value={draft.minimumOrders}
                onChange={(event) =>
                  update("minimumOrders", Number(event.target.value))
                }
              />
            </label>
            <label>
              Completion %
              <input
                inputMode="decimal"
                value={draft.minimumCompletionPercent}
                onChange={(event) =>
                  update("minimumCompletionPercent", event.target.value)
                }
              />
            </label>
            <label>
              Positive %
              <input
                inputMode="decimal"
                value={draft.minimumPositivePercent}
                onChange={(event) =>
                  update("minimumPositivePercent", event.target.value)
                }
              />
            </label>
          </div>
          <label className="check-control">
            <input
              type="checkbox"
              checked={draft.proOnly}
              onChange={(event) => update("proOnly", event.target.checked)}
            />{" "}
            Pro merchants only
          </label>
        </fieldset>
        <fieldset>
          <legend>Result and price boundaries</legend>
          <div className="dialog-field-grid three">
            <label>
              Results per side
              <input
                type="number"
                min="20"
                max="1000"
                step="1"
                value={draft.resultsTarget}
                onChange={(event) =>
                  update("resultsTarget", Number(event.target.value))
                }
              />
            </label>
            <label>
              Maximum Buy price
              <input
                inputMode="decimal"
                value={draft.maximumBuyPrice ?? ""}
                placeholder="No maximum"
                onChange={(event) =>
                  update("maximumBuyPrice", event.target.value || null)
                }
              />
            </label>
            <label>
              Minimum Sell price
              <input
                inputMode="decimal"
                value={draft.minimumSellPrice ?? ""}
                placeholder="No minimum"
                onChange={(event) =>
                  update("minimumSellPrice", event.target.value || null)
                }
              />
            </label>
          </div>
        </fieldset>
        <div className="dialog-footer">
          <p>
            Inputs are never silently clamped. Apply validates and either
            persists the exact draft or returns an actionable error.
          </p>
          <Button variant="primary" onClick={() => dialogRef.current?.close()}>
            Done
          </Button>
        </div>
      </div>
    </dialog>
  );
}
