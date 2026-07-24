import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { DesktopGrid } from "../components/DesktopGrid";

interface DesktopWin {
  slot: string;
  title: string;
}

export function DesktopPage() {
  const [wins, setWins] = useState<DesktopWin[]>([]);

  const refresh = () =>
    invoke<DesktopWin[]>("list_desktop_windows")
      .then(setWins)
      .catch(() => {});

  useEffect(() => {
    // Inherit open windows; IM process management is left to user action.
    invoke<number>("adopt_existing_windows")
      .then(() => refresh())
      .catch(() => {});
    const id = setInterval(refresh, 2000);
    return () => clearInterval(id);
  }, []);

  const focus = (slot: string) =>
    invoke("focus_desktop_window", { slot })
      .then(refresh)
      .catch(() => {});

  const close = (slot: string) =>
    invoke("close_app", { slot })
      .then(refresh)
      .catch(() => {});

  const adopt = () =>
    invoke<number>("adopt_existing_windows")
      .then(() => refresh())
      .catch(() => {});

  const quit = () => {
    if (window.confirm("退出 Winux-Kate？将还原窗口并恢复 explorer。")) {
      invoke("quit_app").catch(() => {});
    }
  };

  return (
    <div className="page desktop">
      <div className="desktop-canvas">
        <DesktopGrid />
      </div>
      <div className="taskbar">
        {wins.map((w) => (
          <div
            key={w.slot}
            className="tb-item"
            onClick={() => focus(w.slot)}
            title={w.title}
          >
            <span className="tb-title">{w.title || "window"}</span>
            <button
              className="tb-close"
              onClick={(e) => {
                e.stopPropagation();
                close(w.slot);
              }}
            >
              ×
            </button>
          </div>
        ))}
        <button className="tb-item tb-adopt" onClick={adopt}>
          + 收纳窗口
        </button>
        <span className="tb-credit">由 Deaicup 工作室制作</span>
        <button className="tb-item tb-quit" onClick={quit} title="退出 Winux-Kate">
          ⏻ 退出
        </button>
      </div>
    </div>
  );
}
