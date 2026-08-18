import type { MarketContextDraft } from "../ipc/lifecycle-contracts";
import { Button, Chip, FieldShell } from "./primitives";
import { Icon } from "./Icon";

export function ContextBar({
  draft,
  unapplied,
  disabled,
  onDraftChange,
  onOpenFilters,
  onApply,
}: {
  readonly draft: MarketContextDraft;
  readonly unapplied: boolean;
  readonly disabled: boolean;
  readonly onDraftChange: (draft: MarketContextDraft) => void;
  readonly onOpenFilters: () => void;
  readonly onApply: () => void;
}) {
  const update = <Key extends keyof MarketContextDraft>(
    key: Key,
    value: MarketContextDraft[Key],
  ) => onDraftChange({ ...draft, [key]: value });
  const payments =
    draft.selectedPaymentMethods.length === 0
      ? "All available methods"
      : draft.selectedPaymentMethods.join(" · ");
  return (
    <section className="context-bar" aria-label="Shared decision context">
      <FieldShell label="Market pair" meta="Validated">
        <button
          className="field-control pair-control"
          type="button"
          onClick={onOpenFilters}
          disabled={disabled}
          aria-label={`Market pair ${draft.asset} ${draft.fiat}. Open context settings`}
        >
          <span className="pair-token">
            <span className="asset-coin" aria-hidden="true">
              ₮
            </span>
            <span>{draft.asset}</span>
            <span className="field-separator">/</span>
            <span>{draft.fiat}</span>
          </span>
          <Icon name="chevron" />
        </button>
      </FieldShell>
      <FieldShell label="Transaction amount">
        <span className="field-control amount-control">
          <input
            aria-label="Transaction amount"
            inputMode="decimal"
            value={draft.amount}
            disabled={disabled}
            onChange={(event) => update("amount", event.target.value)}
          />
          <select
            aria-label="Amount unit"
            value={draft.amountMode}
            disabled={disabled}
            onChange={(event) =>
              update(
                "amountMode",
                event.target.value as MarketContextDraft["amountMode"],
              )
            }
          >
            <option value="fiat">{draft.fiat}</option>
            <option value="asset">{draft.asset}</option>
          </select>
        </span>
      </FieldShell>
      <FieldShell label="Payment context" meta={draft.paymentLogic}>
        <button
          className="field-control pair-control"
          type="button"
          onClick={onOpenFilters}
          disabled={disabled}
          aria-label={`${draft.paymentLogic} payment context. Open filters`}
        >
          <span className="truncate">{payments}</span>
          <Icon name="chevron" />
        </button>
      </FieldShell>
      <FieldShell label="Merchant filters">
        <button
          className="field-control pair-control"
          type="button"
          onClick={onOpenFilters}
          disabled={disabled}
          aria-label="Open merchant filters"
        >
          <span className="filter-summary">
            <strong>{draft.minimumCompletionPercent}%+</strong> completion{" "}
            <span aria-hidden="true">•</span>{" "}
            <strong>{draft.minimumOrders}+</strong> orders
          </span>
          <Icon name="chevron" />
        </button>
      </FieldShell>
      <div className="context-actions">
        <Button icon="filter" disabled={disabled} onClick={onOpenFilters}>
          More filters
        </Button>
        <Button variant="primary" disabled={disabled} onClick={onApply}>
          Apply
        </Button>
      </div>
      {unapplied ? (
        <div className="unapplied-status" role="status">
          <Chip tone="warning">Unapplied changes</Chip>
          <span>Live pages still use the last applied context.</span>
        </div>
      ) : null}
    </section>
  );
}
