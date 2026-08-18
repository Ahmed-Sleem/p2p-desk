import type { ReactNode } from "react";

export type IconName =
  | "brand"
  | "panel"
  | "overview"
  | "offers"
  | "analysis"
  | "history"
  | "health"
  | "settings"
  | "refresh"
  | "filter"
  | "chevron"
  | "info"
  | "empty"
  | "warning"
  | "database"
  | "copy"
  | "edit"
  | "close"
  | "check";

const paths: Readonly<Record<IconName, ReactNode>> = {
  brand: (
    <>
      <path d="M4 13h16c-.4 2.6-2.3 4.7-5 5.6V20H9v-1.4C6.3 17.7 4.4 15.6 4 13Z" />
      <path d="M6 13a2 2 0 0 1-.9-3.6A2.5 2.5 0 0 1 9.6 7a2.8 2.8 0 0 1 5.4-1 2.5 2.5 0 0 1 3.7 3.1A2 2 0 0 1 18 13" />
    </>
  ),
  panel: (
    <>
      <rect x="3" y="4" width="18" height="16" rx="2.5" />
      <path d="M9.5 4v16" />
    </>
  ),
  overview: (
    <>
      <rect x="3" y="3" width="7" height="7" rx="2" />
      <rect x="14" y="3" width="7" height="7" rx="2" />
      <rect x="3" y="14" width="7" height="7" rx="2" />
      <rect x="14" y="14" width="7" height="7" rx="2" />
    </>
  ),
  offers: (
    <>
      <path d="M4 5h16M4 12h16M4 19h16" />
      <circle cx="8" cy="5" r="1.5" />
      <circle cx="16" cy="12" r="1.5" />
      <circle cx="10" cy="19" r="1.5" />
    </>
  ),
  analysis: <path d="M4 19V9m6 10V5m6 14v-7m4 7H2" />,
  history: (
    <>
      <path d="M3 12a9 9 0 1 0 3-6.7L3 8" />
      <path d="M3 3v5h5M12 7v5l3 2" />
    </>
  ),
  health: (
    <>
      <path d="M3 12h4l2.2-5 4.1 10 2.2-5H21" />
      <path d="M12 22C6 19 3 15.8 3 10a5 5 0 0 1 9-3 5 5 0 0 1 9 3c0 5.8-3 9-9 12Z" />
    </>
  ),
  settings: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3A1.7 1.7 0 0 0 14 21v.2h-4V21a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14h-.2v-4H3a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1A1.7 1.7 0 0 0 9 4.6 1.7 1.7 0 0 0 10 3v-.2h4V3a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1h.2v4H21a1.7 1.7 0 0 0-1.6 1Z" />
    </>
  ),
  refresh: (
    <>
      <path d="M3 12a9 9 0 0 1 15.7-6.3L21 8" />
      <path d="M21 3v5h-5M21 12a9 9 0 0 1-15.7 6.3L3 16" />
      <path d="M8 16H3v5" />
    </>
  ),
  filter: <path d="M4 6h16M7 12h10m-7 6h4" />,
  chevron: <path d="m9 7 5 5-5 5" />,
  info: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 11v6M12 7h.01" />
    </>
  ),
  empty: (
    <>
      <path d="M4 7h16v13H4zM7 3h10v4" />
      <path d="M8 12h8M8 16h5" />
    </>
  ),
  warning: (
    <>
      <path d="M12 3 2.5 20h19Z" />
      <path d="M12 9v5M12 17h.01" />
    </>
  ),
  database: (
    <>
      <ellipse cx="12" cy="5" rx="8" ry="3" />
      <path d="M4 5v6c0 1.7 3.6 3 8 3s8-1.3 8-3V5M4 11v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6" />
    </>
  ),
  copy: (
    <>
      <rect x="8" y="8" width="12" height="12" rx="2" />
      <path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" />
    </>
  ),
  edit: (
    <>
      <path d="M4 20h4l11-11-4-4L4 16v4Z" />
      <path d="m13.5 6.5 4 4" />
    </>
  ),
  close: <path d="m6 6 12 12M18 6 6 18" />,
  check: <path d="m5 12 4 4L19 6" />,
};

export function Icon({
  name,
  className = "icon",
}: {
  readonly name: IconName;
  readonly className?: string;
}) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      aria-hidden="true"
      focusable="false"
    >
      {paths[name]}
    </svg>
  );
}
