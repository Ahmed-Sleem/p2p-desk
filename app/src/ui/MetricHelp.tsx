import { useId, useRef } from "react";
import { IconButton } from "./primitives";

export interface MetricHelpEntry {
  readonly title: string;
  readonly meaning: string;
  readonly calculation: string;
  readonly exclusions: string;
}

export function MetricHelp({ entry }: { readonly entry: MetricHelpEntry }) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  return (
    <>
      <IconButton
        icon="info"
        label={`Explain ${entry.title}`}
        onClick={() => dialogRef.current?.showModal()}
      />
      <dialog
        ref={dialogRef}
        aria-labelledby={titleId}
        onClick={(event) => {
          if (event.target === event.currentTarget) event.currentTarget.close();
        }}
      >
        <div className="dialog-head">
          <div>
            <span className="dialog-kicker">Metric help</span>
            <h2 id={titleId}>{entry.title}</h2>
          </div>
          <IconButton
            icon="close"
            label="Close metric explanation"
            autoFocus
            onClick={() => dialogRef.current?.close()}
          />
        </div>
        <div className="dialog-body">
          <dl className="help-list">
            <div>
              <dt>What it means</dt>
              <dd>{entry.meaning}</dd>
            </div>
            <div>
              <dt>How calculated</dt>
              <dd>{entry.calculation}</dd>
            </div>
            <div>
              <dt>Excluded</dt>
              <dd>{entry.exclusions}</dd>
            </div>
          </dl>
          <p className="read-only-note">
            <strong>Read-only:</strong> P2P Desk explains observations and
            calculations. It never prepares, hands off, or executes an order.
          </p>
        </div>
      </dialog>
    </>
  );
}
