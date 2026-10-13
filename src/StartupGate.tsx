import { useEffect, useState } from "react";
import { HashRouter } from "react-router";
import { App } from "./App";
import { Backup, RestoreSection } from "./Backup";
import { readCommandResult } from "./lib/ipc";
import { errorText } from "./lib/recordFeedback";
import {
  classificationLabels,
  getStartupStatus,
  type StartupReport,
} from "./lib/startup";

export function StartupGate() {
  const [report, setReport] = useState<StartupReport | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    void getStartupStatus()
      .then(readCommandResult)
      .then(
        (value) => {
          if (active) setReport(value);
        },
        (error: unknown) => {
          if (active) setError(errorText(error));
        },
      );
    return () => {
      active = false;
    };
  }, []);
  if (report?.ready)
    return (
      <HashRouter>
        <App startup={report} />
      </HashRouter>
    );
  return (
    <main className="startup-screen" tabIndex={-1}>
      <section className="startup-card" aria-labelledby="startup-title">
        <div className="brand">
          <img src="./icon.svg" alt="" width="30" height="30" />
          <p className="brand-name">Shift Happens</p>
        </div>
        <h1 id="startup-title">
          {report || error ? "Startup stopped" : "Opening your local database…"}
        </h1>
        {!report && !error && (
          <p className="status-line" role="status">
            Checking the database before opening the workspace.
          </p>
        )}
        {error && (
          <p className="notice notice-error" role="alert">
            {error} Close and reopen the desktop application.
          </p>
        )}
        {report && (
          <>
            <div className="notice notice-error" role="alert">
              {report.error && (
                <p>
                  {report.error.message} ({report.error.code})
                  {report.error.context.stage
                    ? `, during ${report.error.context.stage.replaceAll("_", " ")}`
                    : ""}
                </p>
              )}
              {report.error?.context.recovery_path && (
                <p className="literal">{report.error.context.recovery_path}</p>
              )}
              {report.reasons.map((reason) => (
                <p key={reason}>{classificationLabels[reason]}</p>
              ))}
              {report.pre_migration_backup.error && (
                <p>{report.pre_migration_backup.error.message}</p>
              )}
            </div>
            <dl className="facts rule-top">
              <dt>Installed version</dt>
              <dd>{report.version}</dd>
              <dt>Database path</dt>
              <dd className="literal">
                {report.database_path ??
                  "The application data directory could not be resolved."}
              </dd>
              <dt>Classification</dt>
              <dd>
                {classificationLabels[report.classification]} (
                {report.classification})
              </dd>
              <dt>Pre-migration backup</dt>
              <dd>
                {report.pre_migration_backup.status === "success"
                  ? "Saved and independently validated"
                  : report.pre_migration_backup.status === "failed"
                    ? "Failed — upgrade was not authorized"
                    : "Not attempted"}
              </dd>
              {report.pre_migration_backup.path && (
                <>
                  <dt>Backup path</dt>
                  <dd className="literal">
                    {report.pre_migration_backup.path}
                  </dd>
                </>
              )}
            </dl>
            <p className="fine-print rule-top">
              Close the app and preserve the database and any validated backup
              for developer inspection. A preservation copy leaves startup
              blocked.
            </p>
            {report.warnings.map((warning, index) => (
              <p className="notice notice-warn" key={index}>
                {warning.message}
              </p>
            ))}
            {!report.preservation_available && (
              <p className="fine-print">
                Preservation backup unavailable:{" "}
                {report.preservation_unavailable_reason}
              </p>
            )}
          </>
        )}
      </section>
      {report?.preservation_available && (
        <Backup
          preservationFilename={
            report.preservation_filename ?? "shift-happens-preservation.sqlite"
          }
        />
      )}
      {report && report.database_path !== null && (
        <RestoreSection
          blocked
          preservationFirst={report.preservation_available}
        />
      )}
    </main>
  );
}
