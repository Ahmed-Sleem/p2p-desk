import type { ButtonHTMLAttributes, ReactNode } from "react";
import { Icon, type IconName } from "./Icon";

export function Button({
  variant = "secondary",
  icon,
  children,
  className = "",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  readonly variant?: "primary" | "secondary" | "quiet" | "danger";
  readonly icon?: IconName;
}) {
  return (
    <button
      className={`button button-${variant} ${className}`.trim()}
      type="button"
      {...props}
    >
      {icon ? <Icon name={icon} /> : null}
      {children}
    </button>
  );
}

export function IconButton({
  label,
  icon,
  className = "",
  ...props
}: Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> & {
  readonly label: string;
  readonly icon: IconName;
}) {
  return (
    <button
      className={`icon-button ${className}`.trim()}
      type="button"
      aria-label={label}
      title={label}
      {...props}
    >
      <Icon name={icon} />
    </button>
  );
}

export function Panel({
  title,
  description,
  action,
  children,
  className = "",
}: {
  readonly title: string;
  readonly description?: string;
  readonly action?: ReactNode;
  readonly children: ReactNode;
  readonly className?: string;
}) {
  return (
    <section className={`panel ${className}`.trim()}>
      <header className="panel-header">
        <div className="panel-title">
          <h2>{title}</h2>
          {description ? <p>{description}</p> : null}
        </div>
        {action ? <div className="panel-actions">{action}</div> : null}
      </header>
      <div className="panel-body">{children}</div>
    </section>
  );
}

export function Chip({
  children,
  tone = "neutral",
}: {
  readonly children: ReactNode;
  readonly tone?: "neutral" | "success" | "warning" | "dark";
}) {
  return <span className={`chip chip-${tone}`}>{children}</span>;
}

export function FieldShell({
  label,
  meta,
  children,
  className = "",
}: {
  readonly label: string;
  readonly meta?: ReactNode;
  readonly children: ReactNode;
  readonly className?: string;
}) {
  return (
    <div className={`field ${className}`.trim()}>
      <div className="field-label">
        <span>{label}</span>
        {meta ? <span>{meta}</span> : null}
      </div>
      {children}
    </div>
  );
}

export function Skeleton({
  kind = "line",
  label,
}: {
  readonly kind?: "line" | "block" | "table";
  readonly label?: string;
}) {
  return (
    <div className={`skeleton skeleton-${kind}`} aria-hidden="true">
      {label ? <span className="visually-hidden">{label}</span> : null}
    </div>
  );
}
