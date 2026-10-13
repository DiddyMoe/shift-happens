import {
  useEffect,
  useEffectEvent,
  useLayoutEffect,
  useRef,
  useState,
  type MouseEvent,
  type ReactNode,
} from "react";
import {
  Link,
  Navigate,
  NavLink,
  Route,
  Routes,
  useLocation,
} from "react-router";
import { trapDialogFocus } from "./lib/dialogFocus";
import { errorText } from "./lib/recordFeedback";
import { HelpContent, HelpQuickReference } from "./HelpContent";
import { applyTheme, rememberTheme, rememberedTheme } from "./lib/theme";
import { Backup } from "./Backup";
import { AllSchedules } from "./AllSchedules";
import { ScheduleWorkspace } from "./schedule/ScheduleWorkspace";
import { Holidays } from "./Holidays";
import { Settings } from "./Settings";
import { About } from "./About";
import { AvailabilityPto } from "./AvailabilityPto";
import { Employees } from "./Employees";
import { PositionsCoverage } from "./PositionsCoverage";
import { ShiftTemplates } from "./ShiftTemplates";
import {
  getSettings,
  readCommandResult,
  type ThemePreference,
} from "./lib/ipc";
import { legacyRoutes, managerRoutes, type ManagerRoute } from "./routes";
import type { StartupReport } from "./lib/startup";

function NotFoundPage() {
  return (
    <section className="page page-narrow" aria-labelledby="page-title">
      <header className="page-header">
        <div className="page-heading">
          <h1 id="page-title">This screen does not exist</h1>
          <p className="page-lede">
            Choose a screen from the navigation or go to Schedule.
          </p>
        </div>
      </header>
      <Link className="btn btn-primary" to="/">
        Go to Schedule
      </Link>
    </section>
  );
}

/** Old screen addresses, merged into Schedule, open the matching plan or
 * dialog there. */
function LegacyRedirect({ path }: { path: (typeof legacyRoutes)[number] }) {
  const { search } = useLocation();
  const old = new URLSearchParams(search);
  const next = new URLSearchParams();
  const id = old.get("id");
  if (id) next.set("id", id);
  const assignment = old.get("assignment");
  if (path === "/schedule-editor" && assignment)
    next.set("assignment", assignment);
  if (path === "/validation") next.set("open", "issues");
  if (path === "/generate-schedule") next.set("open", "generate");
  const query = next.toString();
  return <Navigate replace to={query ? `/?${query}` : "/"} />;
}

const navGroups: readonly {
  label: string | null;
  keys: readonly ManagerRoute["key"][];
}[] = [
  { label: null, keys: ["schedule", "schedules"] },
  { label: "People", keys: ["employees", "availability_pto"] },
  {
    label: "Setup",
    keys: ["shift_templates", "positions_coverage", "holidays"],
  },
];
// Pinned to the foot of the sidebar, apart from the planning workflow.
const utilityKeys: readonly ManagerRoute["key"][] = ["backup", "settings"];

function NavItems({ keys }: { keys: readonly ManagerRoute["key"][] }) {
  return keys.map((key) => {
    const route = managerRoutes.find((item) => item.key === key);
    return (
      route && (
        <li key={route.key}>
          <NavLink
            to={route.path}
            end
            className={({ isActive }) => `nav-link${isActive ? " active" : ""}`}
          >
            {route.label}
          </NavLink>
        </li>
      )
    );
  });
}

function HelpIcon() {
  return (
    <svg className="icon" viewBox="0 0 16 16" aria-hidden="true">
      <circle cx="8" cy="8" r="6.5" fill="none" stroke="currentColor" />
      <path
        d="M6.2 6.3a1.9 1.9 0 0 1 3.7.4c0 1.3-1.9 1.6-1.9 2.8"
        fill="none"
        stroke="currentColor"
        strokeLinecap="round"
      />
      <circle cx="8" cy="11.6" r="0.8" fill="currentColor" />
    </svg>
  );
}

export function App({ startup }: { startup: StartupReport }) {
  const main = useRef<HTMLElement>(null);
  const help = useRef<HTMLDialogElement>(null);
  const { pathname } = useLocation();
  const [themeError, setThemeError] = useState("");
  const [theme, setTheme] = useState<ThemePreference>(rememberedTheme);
  const [settingsVersion, setSettingsVersion] = useState(0);
  const [operationNotice, setOperationNotice] = useState("");
  useEffect(() => {
    let canceled = false;
    void getSettings()
      .then(readCommandResult)
      .then((settings) => {
        if (!canceled) setThemeError("");
        const value = settings.find(
          (setting) => setting.key === "theme_preference",
        )?.value;
        if (
          !canceled &&
          value?.kind === "string" &&
          (value.value === "dark" ||
            value.value === "light" ||
            value.value === "system")
        ) {
          setTheme(value.value);
          rememberTheme(value.value);
        }
      })
      .catch((error: unknown) => {
        if (!canceled) setThemeError(errorText(error));
      });
    return () => {
      canceled = true;
    };
  }, [settingsVersion]);
  useEffect(() => {
    applyTheme(theme);
    if (theme !== "system") return;
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => applyTheme("system");
    query.addEventListener("change", apply);
    return () => query.removeEventListener("change", apply);
  }, [theme]);
  useEffect(() => {
    function completed(event: Event) {
      const { command, origin, status, message } = (
        event as CustomEvent<{
          command: string;
          origin: string;
          status: string;
          id: string | null;
          message: string | null;
        }>
      ).detail;
      if (
        command === "update_manager_setting" &&
        (status === "success" || status === "no_change")
      )
        setSettingsVersion((v) => v + 1);
      // A change finished after you moved to another screen: say how it went.
      if (origin === window.location.hash) return;
      if (status === "success")
        setOperationNotice("A change you started on another screen was saved.");
      else if (
        status === "failure" ||
        status === "blocked" ||
        status === "validation_error" ||
        status === "stale_revision"
      )
        setOperationNotice(
          `A change you started on another screen wasn't saved.${message ? ` ${message}` : ""}`,
        );
    }
    window.addEventListener("shifthappens:operation", completed);
    return () =>
      window.removeEventListener("shifthappens:operation", completed);
  }, []);

  useLayoutEffect(() => {
    const route = managerRoutes.find((item) => item.path === pathname);
    // Old addresses redirect to Schedule; titling them so avoids a flash.
    const legacy = legacyRoutes.some((path) => path === pathname);
    document.title = `${route?.label ?? (legacy ? "Schedule" : "Page not found")} · Shift Happens`;
    main.current?.focus({ preventScroll: true });
    if (main.current) main.current.scrollTop = 0;
  }, [pathname]);

  const [helpOpen, setHelpOpen] = useState(false);
  function openHelp() {
    if (help.current && !help.current.open) {
      help.current.showModal();
      setHelpOpen(true);
    }
  }
  const openHelpOnF1 = useEffectEvent(openHelp);
  useEffect(() => {
    function keydown(event: KeyboardEvent) {
      if (event.key !== "F1" || event.defaultPrevented) return;
      event.preventDefault();
      openHelpOnF1();
    }
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  }, []);
  const screens: Record<ManagerRoute["key"], ReactNode> = {
    schedule: <ScheduleWorkspace />,
    schedules: <AllSchedules />,
    employees: <Employees />,
    availability_pto: <AvailabilityPto />,
    shift_templates: <ShiftTemplates />,
    positions_coverage: <PositionsCoverage />,
    holidays: <Holidays />,
    backup: <Backup />,
    settings: <Settings />,
    about: <About openHelp={openHelp} startup={startup} />,
  };

  // The backdrop belongs to the dialog element, so a press counts as outside
  // only when it lands beyond the dialog box. Requiring both the press and the
  // release outside keeps a text selection dragged past the edge from closing.
  const helpPressOutside = useRef(false);
  function outsideHelp(event: MouseEvent<HTMLDialogElement>) {
    const box = event.currentTarget.getBoundingClientRect();
    return (
      event.target === event.currentTarget &&
      (event.clientX < box.left ||
        event.clientX > box.right ||
        event.clientY < box.top ||
        event.clientY > box.bottom)
    );
  }

  return (
    <>
      <div className="app-shell">
        <button
          type="button"
          className="skip-link"
          onClick={() => main.current?.focus()}
        >
          Skip to main content
        </button>
        <aside className="sidebar">
          <div className="brand">
            <img src="./icon.svg" alt="" width="30" height="30" />
            <p className="brand-name">Shift Happens</p>
          </div>
          <nav aria-label="Main navigation">
            {navGroups.map((group) => (
              <div className="nav-group" key={group.keys[0]}>
                {group.label && (
                  <p className="nav-group-label" id={`nav-${group.keys[0]}`}>
                    {group.label}
                  </p>
                )}
                <ul
                  className="nav-list"
                  aria-labelledby={
                    group.label ? `nav-${group.keys[0]}` : undefined
                  }
                >
                  <NavItems keys={group.keys} />
                </ul>
              </div>
            ))}
            <ul className="nav-list nav-utility">
              <NavItems keys={utilityKeys} />
            </ul>
          </nav>
          <div className="sidebar-footer">
            <button type="button" className="btn btn-quiet" onClick={openHelp}>
              <HelpIcon />
              Help
            </button>
            <NavLink
              to="/about"
              end
              className={({ isActive }) =>
                `sidebar-about${isActive ? " active" : ""}`
              }
            >
              About · Version {__APP_VERSION__}
            </NavLink>
          </div>
        </aside>
        <main
          id="main-content"
          ref={main}
          tabIndex={-1}
          className="main-content"
        >
          {(themeError || operationNotice) && (
            <div className="shell-notices">
              {themeError && (
                <p className="notice notice-error" role="alert">
                  <span>
                    Saved appearance could not be loaded: {themeError}{" "}
                    <Link to="/settings">Review Settings</Link>
                  </span>
                </p>
              )}
              {operationNotice && (
                <div className="notice notice-info" role="status">
                  <p>{operationNotice}</p>
                  <button
                    className="btn btn-sm"
                    type="button"
                    onClick={() => setOperationNotice("")}
                  >
                    Dismiss
                  </button>
                </div>
              )}
            </div>
          )}
          <Routes>
            {managerRoutes.map((route) => (
              <Route
                key={route.key}
                path={route.path}
                element={screens[route.key]}
              />
            ))}
            {legacyRoutes.map((path) => (
              <Route
                key={path}
                path={path}
                element={<LegacyRedirect path={path} />}
              />
            ))}
            <Route path="*" element={<NotFoundPage />} />
          </Routes>
        </main>
      </div>
      <dialog
        ref={help}
        className="help-dialog"
        aria-labelledby="help-title"
        onKeyDown={trapDialogFocus}
        onClose={() => setHelpOpen(false)}
        onMouseDown={(event) => {
          helpPressOutside.current = outsideHelp(event);
        }}
        onClick={(event) => {
          if (helpPressOutside.current && outsideHelp(event))
            help.current?.close();
          helpPressOutside.current = false;
        }}
      >
        <header className="help-header">
          <h2 id="help-title">Help</h2>
          <div className="cluster">
            <button
              type="button"
              className="btn"
              onClick={() => window.print()}
            >
              Print quick reference
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => help.current?.close()}
            >
              Close
            </button>
          </div>
        </header>
        {helpOpen && <HelpContent onNavigate={() => help.current?.close()} />}
        <HelpQuickReference />
      </dialog>
    </>
  );
}
