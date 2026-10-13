import { useState } from "react";
import { useCommandQuery } from "./hooks/useCommandQuery";
import { getDatabaseHealth, type DatabaseHealth } from "./lib/ipc";
import { limitations } from "./lib/help";
import { classificationLabels, type StartupReport } from "./lib/startup";
import { LoadError } from "./RecordControls";

function HealthPanel({
  health,
  error,
  onRetry,
}: {
  health: DatabaseHealth | null;
  error: string;
  onRetry: () => void;
}) {
  if (error) return <LoadError message={error} onRetry={onRetry} />;
  if (health === null)
    return (
      <p className="status-line" role="status">
        Reading database health…
      </p>
    );
  return (
    <>
      <dl className="facts">
        <dt>Health</dt>
        <dd>
          <span
            className={`tag ${health.health === "ok" ? "tag-ok" : "tag-danger"}`}
          >
            {health.health === "ok" ? "OK" : "Needs attention"}
          </span>
        </dd>
        <dt>Migrations</dt>
        <dd>
          <span
            className={`tag ${health.migrations.current ? "tag-ok" : "tag-warn"}`}
          >
            {health.migrations.current ? "Current" : "Needs attention"}
          </span>
        </dd>
        <dt>Database path</dt>
        <dd className="literal">{health.path}</dd>
      </dl>
      <details className="set-technical">
        <summary>Technical details</summary>
        <dl className="facts">
          <dt>File exists</dt>
          <dd>{health.exists ? "Yes" : "No"}</dd>
          <dt>Foreign keys</dt>
          <dd>{health.foreign_keys ? "Enabled" : "Disabled"}</dd>
          <dt>Journal mode</dt>
          <dd>{health.journal_mode}</dd>
          <dt>Synchronous mode</dt>
          <dd>{health.synchronous}</dd>
          <dt>Busy timeout / pool limit</dt>
          <dd>
            <span className="display">{health.busy_timeout_ms}</span> ms /{" "}
            <span className="display">{health.max_pool_connections}</span>{" "}
            connections
          </dd>
          <dt>SQLite engine</dt>
          <dd className="display">{health.engine.version}</dd>
          <dt>SQLite source ID</dt>
          <dd className="literal">{health.engine.source_id}</dd>
        </dl>
        <p className="fine-print set-note">
          Read-only inspection of the live database, including its committed WAL
          data.
        </p>
      </details>
    </>
  );
}

export function About({
  openHelp,
  startup,
}: {
  openHelp: () => void;
  startup: StartupReport;
}) {
  const health = useCommandQuery("database-health", getDatabaseHealth);
  return (
    <section className="page page-narrow" aria-labelledby="page-title">
      <header className="page-header">
        <div className="page-heading">
          <h1 id="page-title">About</h1>
          <p className="page-lede">
            This copy of Shift Happens and its local database.
          </p>
        </div>
        <div className="page-actions">
          <button type="button" className="btn" onClick={openHelp}>
            Help
          </button>
        </div>
      </header>
      {startup.warnings.map((warning, index) => (
        <p className="notice notice-warn" key={index}>
          {warning.message}
        </p>
      ))}
      <section className="set-group">
        <h2>Shift Happens</h2>
        <dl className="facts">
          <dt>Version</dt>
          <dd className="display">{__APP_VERSION__}</dd>
          {startup.version !== __APP_VERSION__ && (
            <>
              <dt>Installed version</dt>
              <dd className="display">{startup.version}</dd>
            </>
          )}
          <dt>Database at startup</dt>
          <dd>
            {startup.classification === "pending"
              ? "Upgraded to this version"
              : classificationLabels[startup.classification]}
          </dd>
          {startup.pre_migration_backup.status === "success" && (
            <>
              <dt>Backup saved before the upgrade</dt>
              <dd className="literal">{startup.pre_migration_backup.path}</dd>
            </>
          )}
        </dl>
      </section>
      <section className="set-group">
        <h2>Intended use</h2>
        <p className="set-intended-use">
          Shift Happens is an administrative staff-scheduling planning tool. It
          doesn&rsquo;t use patient data, doesn&rsquo;t give clinical
          recommendations, and isn&rsquo;t a medical device. Your
          hospital&rsquo;s official scheduling, timekeeping, and payroll systems
          remain the system of record.
        </p>
      </section>
      <section className="set-group">
        <h2>Database</h2>
        <HealthPanel
          health={health.data}
          error={health.error}
          onRetry={health.reload}
        />
      </section>
      <section className="set-group">
        <h2>Limitations</h2>
        <ul className="bullets set-limitations">
          {limitations.map((limit) => (
            <li key={limit}>{limit}</li>
          ))}
        </ul>
      </section>
      <section className="set-group">
        <h2>Open-source software</h2>
        <p className="set-intended-use">
          Shift Happens is built with open-source packages, each used under its
          own license.
        </p>
        <ThirdPartyNotices />
      </section>
    </section>
  );
}

/** The bundled license notices, loaded only when someone opens them. */
function ThirdPartyNotices() {
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState("");
  function load() {
    if (text !== null) return;
    setError("");
    import("./generated/third-party-notices.txt?raw").then(
      (notices: { default: string }) => setText(notices.default),
      () => setError("The notices could not be loaded. Try again."),
    );
  }
  return (
    <details
      className="about-notices"
      onToggle={(event) => {
        if (event.currentTarget.open) load();
      }}
    >
      <summary>Third-party notices</summary>
      {error ? (
        <p className="notice notice-error" role="alert">
          {error}
        </p>
      ) : text === null ? (
        <p className="status-line" role="status">
          Loading notices…
        </p>
      ) : (
        <pre
          className="about-notices-text"
          tabIndex={0}
          role="region"
          aria-label="Third-party notices"
        >
          {text}
        </pre>
      )}
    </details>
  );
}
