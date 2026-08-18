import type { ReactNode } from "react";
import { Icon } from "./Icon";
import { IconButton } from "./primitives";
import { NAVIGATION, type PageId } from "./content";

export function AppShell({
  page,
  expanded,
  onPageChange,
  onToggle,
  topbarActions,
  children,
}: {
  readonly page: PageId;
  readonly expanded: boolean;
  readonly onPageChange: (page: PageId) => void;
  readonly onToggle: () => void;
  readonly topbarActions?: ReactNode;
  readonly children: ReactNode;
}) {
  const pageLabel =
    NAVIGATION.find((item) => item.id === page)?.label ?? "Overview";
  return (
    <div className={`app-shell${expanded ? " sidebar-expanded" : ""}`}>
      <a className="skip-link" href="#workspace">
        Skip to workspace
      </a>
      <aside
        className="sidebar"
        id="primary-sidebar"
        aria-label="Primary navigation"
      >
        <div className="brand">
          <span className="brand-mark">
            <Icon name="brand" className="brand-logo" />
          </span>
          <span className="brand-copy">P2P Desk</span>
          {expanded ? (
            <IconButton
              className="sidebar-close"
              icon="close"
              label="Close navigation"
              onClick={onToggle}
            />
          ) : null}
        </div>
        <span className="nav-label">Workspace</span>
        <nav aria-label="Primary navigation">
          <ul className="nav-list">
            {NAVIGATION.map((item) => (
              <li key={item.id}>
                <button
                  className="nav-button"
                  type="button"
                  aria-label={item.label}
                  title={item.label}
                  aria-current={item.id === page ? "page" : undefined}
                  onClick={() => onPageChange(item.id)}
                >
                  <Icon name={item.icon} />
                  <span>{item.label}</span>
                </button>
              </li>
            ))}
          </ul>
        </nav>
      </aside>
      <div className="main-shell">
        <header className="topbar">
          <IconButton
            className="sidebar-toggle"
            icon="panel"
            label={expanded ? "Collapse sidebar" : "Expand sidebar"}
            aria-expanded={expanded}
            aria-controls="primary-sidebar"
            onClick={onToggle}
          />
          <div className="page-heading">
            <h1>{pageLabel}</h1>
          </div>
          <div className="topbar-actions">{topbarActions}</div>
        </header>
        <main className="workspace-scroll" id="workspace" tabIndex={-1}>
          <div className="workspace-content">{children}</div>
        </main>
      </div>
    </div>
  );
}
