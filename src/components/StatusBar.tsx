import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface SystemStatus {
  time: string;
  date: string;
  volume: number;
  muted: boolean;
  brightness: number;
  bluetooth_on: boolean;
  wifi_ssid: string;
  wifi_connected: boolean;
}

export function StatusBar() {
  const [s, setS] = useState<SystemStatus | null>(null);
  const [vol, setVol] = useState(0);
  const [bright, setBright] = useState(100);
  const dragVol = useRef(false);
  const dragBright = useRef(false);
  const volTimer = useRef<number | null>(null);
  const brightTimer = useRef<number | null>(null);

  useEffect(() => {
    const tick = () =>
      invoke<SystemStatus>("system_status")
        .then((st) => {
          setS(st);
          if (!dragVol.current) setVol(Math.round(st.volume * 100));
          if (!dragBright.current)
            setBright(st.brightness <= 100 ? st.brightness : 100);
        })
        .catch(() => {});
    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, []);

  const onVol = (v: number) => {
    setVol(v);
    if (volTimer.current) window.clearTimeout(volTimer.current);
    volTimer.current = window.setTimeout(
      () => invoke("set_volume", { v: v / 100 }).catch(() => {}),
      60
    );
  };

  const onBright = (v: number) => {
    setBright(v);
    if (brightTimer.current) window.clearTimeout(brightTimer.current);
    // brightness shells out to PowerShell/WMI which is slow -> debounce hard.
    brightTimer.current = window.setTimeout(
      () => invoke("set_brightness", { v }).catch(() => {}),
      350
    );
  };

  if (!s) return <div className="statusbar" />;

  return (
    <div className="statusbar">
      <span className="sb-item clock">
        {s.date} · {s.time}
      </span>
      <span className="sb-item">
        VOL
        <input
          type="range"
          min={0}
          max={100}
          value={vol}
          onPointerDown={() => (dragVol.current = true)}
          onPointerUp={() => (dragVol.current = false)}
          onInput={(e) => onVol(Number(e.currentTarget.value))}
        />
        <b style={{ color: s.muted ? "var(--danger)" : "var(--accent2)" }}>
          {s.muted ? "MUTE" : `${vol}%`}
        </b>
      </span>
      <span className="sb-item">
        BRIGHT
        <input
          type="range"
          min={0}
          max={100}
          value={bright}
          onPointerDown={() => (dragBright.current = true)}
          onPointerUp={() => (dragBright.current = false)}
          onInput={(e) => onBright(Number(e.currentTarget.value))}
        />
        <b style={{ color: "var(--accent2)" }}>{bright}%</b>
      </span>
      <span
        className="sb-item"
        style={{ color: s.bluetooth_on ? "var(--accent2)" : "var(--text-dim)" }}
      >
        BT {s.bluetooth_on ? "ON" : "OFF"}
      </span>
      <span
        className="sb-item"
        style={{ color: s.wifi_connected ? "var(--accent2)" : "var(--text-dim)" }}
      >
        WIFI {s.wifi_connected ? s.wifi_ssid : "--"}
      </span>
      <span className="sb-item hint" style={{ marginLeft: "auto" }}>
        Ctrl+Tab 切页 · Ctrl+Shift+Tab 上下文
      </span>
    </div>
  );
}
