import { useEffect, useRef } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";

interface Props {
  cmd?: string;
}

/** A single xterm.js terminal backed by a ConPTY session in the Rust backend. */
export function Terminal({ cmd = "powershell.exe" }: Props) {
  const hostRef = useRef<HTMLDivElement>(null);
  const pidRef = useRef<number | null>(null);

  useEffect(() => {
    if (!hostRef.current) return;
    const term = new XTerm({
      fontFamily: "'JetBrains Mono', monospace",
      fontSize: 13,
      cursorBlink: true,
      theme: {
        background: "#04060f",
        foreground: "#cfeffb",
        cursor: "#00e5ff",
        selectionBackground: "rgba(0,229,255,0.3)",
        black: "#04060f",
        green: "#39ff14",
        cyan: "#00e5ff",
        blue: "#2b8fff",
        yellow: "#ffcc00",
        red: "#ff4d6d",
      },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(hostRef.current);
    fit.fit();

    const queued: string[] = [];
    term.onData((data) => {
      if (pidRef.current !== null) {
        invoke("pty_write", { id: pidRef.current, data }).catch(console.error);
      } else {
        queued.push(data);
      }
    });
    term.onResize(({ cols, rows }) => {
      if (pidRef.current !== null) {
        invoke("pty_resize", { id: pidRef.current, cols, rows }).catch(
          console.error
        );
      }
    });

    let unlisten: UnlistenFn | null = null;
    listen<{ id: number; data: string }>("pty-data", (e) => {
      if (e.payload.id === pidRef.current) {
        const bytes = Uint8Array.from(atob(e.payload.data), (c) =>
          c.charCodeAt(0)
        );
        term.write(bytes);
      }
    }).then((u) => {
      unlisten = u;
    });

    invoke<number>("pty_spawn", {
      cmd,
      cols: term.cols,
      rows: term.rows,
    })
      .then((pid) => {
        pidRef.current = pid;
        while (queued.length) {
          const d = queued.shift()!;
          invoke("pty_write", { id: pid, data: d }).catch(console.error);
        }
      })
      .catch((e) => term.writeln(`\x1b[31m[spawn error] ${String(e)}\x1b[0m`));

    const ro = new ResizeObserver(() => {
      try {
        fit.fit();
      } catch {
        /* ignore */
      }
    });
    ro.observe(hostRef.current);

    return () => {
      ro.disconnect();
      if (unlisten) unlisten();
      if (pidRef.current !== null) {
        invoke("pty_kill", { id: pidRef.current }).catch(() => {});
      }
      term.dispose();
    };
  }, [cmd]);

  return <div className="terminal-host" ref={hostRef} />;
}
