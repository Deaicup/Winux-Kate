import { useEffect, useState } from "react";
import { useStore } from "./store";
import { useTauriEvents } from "./hooks/useTauriEvents";
import { useLayoutRects } from "./hooks/useLayoutRects";
import { PageSwitcher } from "./components/PageSwitcher";
import { DashboardPage } from "./pages/DashboardPage";
import { IdePage } from "./pages/IdePage";
import { ImPage } from "./pages/ImPage";
import { DesktopPage } from "./pages/DesktopPage";
import { CustomPageView } from "./pages/CustomPageView";
import ErrorBoundary from "./ErrorBoundary";

const BOOT_LINES: Array<[string, string]> = [
  ["WINUX-KATE SHELL v0.1.0", "ok"],
  ["initializing core runtime...", "dim"],
  ["mounting conpty terminal subsystem............[ OK ]", "ok"],
  ["loading window manager (SetParent/EnumWindows)[ OK ]", "ok"],
  ["registering low-level keyboard hook..........[ OK ]", "ok"],
  ["probing core audio endpoint..................[ OK ]", "ok"],
  ["resolving desktop shortcuts..................[ OK ]", "ok"],
  ["explorer.exe replaced by Winux-Kate shell....[ OK ]", "ok"],
  ["shell ready.", "ok"],
  ["由 Deaicup 工作室制作", "dim"],
];

function BootScreen({ onDone }: { onDone: () => void }) {
  const [shown, setShown] = useState(0);

  useEffect(() => {
    if (shown >= BOOT_LINES.length) {
      const t = setTimeout(onDone, 600);
      return () => clearTimeout(t);
    }
    const t = setTimeout(() => setShown((n) => n + 1), 220);
    return () => clearTimeout(t);
  }, [shown, onDone]);

  return (
    <div className={"boot" + (shown >= BOOT_LINES.length ? " done" : "")}>
      <div className="boot-title">WINUX-KATE</div>
      <div className="boot-subtitle">由 Deaicup 工作室制作</div>
      <div className="boot-lines">
        {BOOT_LINES.slice(0, shown).map((l, i) => (
          <div key={i} className={"line-" + l[1]}>
            &gt; {l[0]}
          </div>
        ))}
      </div>
      <div className="boot-bar">
        <i />
      </div>
      <div className="boot-hint">SYSTEM ONLINE</div>
      <div className="boot-credit">© 2026 Deaicup Studio · 由 Deaicup 工作室制作</div>
    </div>
  );
}

export default function App() {
  useTauriEvents();
  const booted = useStore((s) => s.booted);
  const currentPage = useStore((s) => s.currentPage);
  const imView = useStore((s) => s.imView);
  const customPages = useStore((s) => s.customPages);
  useLayoutRects(currentPage, imView);

  return (
    <ErrorBoundary>
      <div className="app-shell">
        {!booted && <BootScreen onDone={() => useStore.getState().setBooted(true)} />}
        <div className="topbar">
          <span className="brand">⬢ WINUX-KATE</span>
          <span className="brand-credit">由 Deaicup 工作室制作</span>
          <PageSwitcher />
          <span className="spacer" />
          <span className="credit-top">© Deaicup Studio</span>
          <span className="clock">PG {currentPage}</span>
        </div>
        <div className="page-host">
          {/* All pages stay mounted persistently; hidden via CSS so their
              internal state (terminals, launched apps) survives page switches. */}
          <div className={currentPage === 1 ? "page-active" : "page-hidden"}>
            <DashboardPage />
          </div>
          <div className={currentPage === 2 ? "page-active" : "page-hidden"}>
            <IdePage />
          </div>
          <div className={currentPage === 3 ? "page-active" : "page-hidden"}>
            <ImPage />
          </div>
          <div className={currentPage === 4 ? "page-active" : "page-hidden"}>
            <DesktopPage />
          </div>
          {/* Custom pages: render only existing pages, hide non-current ones. */}
          {customPages.map((cp) => (
            <div key={cp.id} className={currentPage === cp.id ? "page-active" : "page-hidden"}>
              <CustomPageView pageId={cp.id} />
            </div>
          ))}
        </div>
      </div>
    </ErrorBoundary>
  );
}
