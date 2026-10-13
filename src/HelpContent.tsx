import { useState } from "react";
import { Link } from "react-router";
import {
  howTo,
  monthlySteps,
  privacy,
  setupSteps,
  shortcuts,
} from "./lib/help";
import { SetupChecklist } from "./schedule/SetupChecklist";

const sections = [
  { key: "start", label: "Getting started" },
  { key: "how", label: "How to" },
  { key: "keys", label: "Keyboard shortcuts" },
  { key: "privacy", label: "Data and privacy" },
  { key: "about", label: "About" },
] as const;
type Section = (typeof sections)[number]["key"];

function Steps({
  steps,
  onNavigate,
}: {
  steps: readonly { path: string; title: string; detail: string }[];
  onNavigate?: () => void;
}) {
  return (
    <ol className="help-workflow">
      {steps.map((step) => (
        <li key={step.title}>
          {onNavigate ? (
            <Link to={step.path} onClick={onNavigate}>
              {step.title}
            </Link>
          ) : (
            <strong>{step.title}</strong>
          )}
          <p>{step.detail}</p>
        </li>
      ))}
    </ol>
  );
}

function ShortcutTable() {
  return (
    <table className="help-keys">
      <tbody>
        {shortcuts.map((shortcut) => (
          <tr key={shortcut.keys}>
            <th scope="row">
              <kbd>{shortcut.keys}</kbd>
            </th>
            <td>{shortcut.action}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Help's sections, one at a time, chosen from the list beside them. */
export function HelpContent({ onNavigate }: { onNavigate: () => void }) {
  const [section, setSection] = useState<Section>("start");
  const label = sections.find((s) => s.key === section)?.label;
  return (
    <div className="help-body">
      <nav className="help-nav" aria-label="Help sections">
        <ul>
          {sections.map((s) => (
            <li key={s.key}>
              <button
                type="button"
                className="help-nav-item"
                aria-current={section === s.key ? "true" : undefined}
                onClick={() => setSection(s.key)}
              >
                {s.label}
              </button>
            </li>
          ))}
        </ul>
      </nav>
      <section className="help-section" aria-labelledby="help-section-title">
        <h3 id="help-section-title">{label}</h3>
        {section === "start" && (
          <>
            <p>
              Set things up once, then build, check, and finalize each schedule
              period. Each item links to its screen.
            </p>
            <SetupChecklist
              variant="full"
              refresh={0}
              heading="Setup status"
              level={4}
              onNavigate={onNavigate}
            />
            <h4>Each schedule period</h4>
            <Steps steps={monthlySteps} onNavigate={onNavigate} />
          </>
        )}
        {section === "how" &&
          howTo.map((topic) => (
            <section key={topic.title} className="help-topic">
              <h4>{topic.title}</h4>
              {topic.text.map((paragraph) => (
                <p key={paragraph}>{paragraph}</p>
              ))}
            </section>
          ))}
        {section === "keys" && <ShortcutTable />}
        {section === "privacy" &&
          privacy.map((topic) => (
            <section key={topic.title} className="help-topic">
              <h4>{topic.title}</h4>
              <p>{topic.text}</p>
            </section>
          ))}
        {section === "about" && (
          <>
            <p>
              Shift Happens {__APP_VERSION__} is an administrative
              staff-scheduling planning tool. It doesn&rsquo;t use patient data
              and isn&rsquo;t a medical device.
            </p>
            <p>
              <Link to="/about" onClick={onNavigate}>
                Open About
              </Link>{" "}
              for the data folder, database health, limitations, and third-party
              notices.
            </p>
          </>
        )}
      </section>
    </div>
  );
}

/** The one-page printed summary; shown only when printing from Help. */
export function HelpQuickReference() {
  return (
    <div className="help-quick-reference">
      <h2>Shift Happens quick reference</h2>
      <h3>Set up once</h3>
      <Steps steps={setupSteps} />
      <h3>Each schedule period</h3>
      <Steps steps={monthlySteps} />
      <h3>Keyboard shortcuts</h3>
      <ShortcutTable />
      <h3>What the labels mean</h3>
      {howTo
        .filter((topic) => topic.title === "What the labels mean")
        .flatMap((topic) => topic.text)
        .map((paragraph) => (
          <p key={paragraph}>{paragraph}</p>
        ))}
      <h3>Data and privacy</h3>
      <p>
        Data stays on this computer. Keep notes free of patient and medical
        details, and keep backups and printouts in hospital-approved places.
      </p>
    </div>
  );
}
