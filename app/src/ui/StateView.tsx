import type { ReactNode } from "react";
import { Icon, type IconName } from "./Icon";

export type StateTone = "loading" | "empty" | "error" | "offline" | "success";

export function StateView({
  tone,
  icon,
  title,
  detail,
  children,
  compact = false,
  live = false,
}: {
  readonly tone: StateTone;
  readonly icon?: IconName;
  readonly title: string;
  readonly detail: string;
  readonly children?: ReactNode;
  readonly compact?: boolean;
  readonly live?: boolean;
}) {
  const resolvedIcon: IconName =
    icon ??
    (tone === "error" || tone === "offline"
      ? "warning"
      : tone === "success"
        ? "check"
        : "empty");
  const role = tone === "error" || tone === "offline" ? "alert" : undefined;
  return (
    <section
      className={`state-view state-${tone}${compact ? " state-compact" : ""}`}
      role={role}
      aria-live={live ? "polite" : undefined}
      aria-busy={tone === "loading" ? true : undefined}
    >
      <div className="state-inner">
        <div className="state-icon">
          <Icon name={resolvedIcon} />
        </div>
        <h2>{title}</h2>
        <p>{detail}</p>
        {children ? <div className="state-actions">{children}</div> : null}
      </div>
    </section>
  );
}
