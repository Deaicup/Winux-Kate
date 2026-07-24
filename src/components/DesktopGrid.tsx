import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Shortcut {
  name: string;
  target: string;
  args: string;
  icon: string;
}

export function DesktopGrid() {
  const [items, setItems] = useState<Shortcut[]>([]);
  const [selected, setSelected] = useState<string | null>(null);

  useEffect(() => {
    invoke<Shortcut[]>("list_shortcuts")
      .then(setItems)
      .catch((e) => console.error(e));
  }, []);

  const launch = (s: Shortcut) => {
    invoke<string>("launch_app", { target: s.target, args: s.args }).catch(
      (e) => alert(`启动失败: ${String(e)}`)
    );
  };

  return (
    <div className="desktop-grid">
      {items.length === 0 && (
        <div className="desktop-empty">桌面上未找到快捷方式</div>
      )}
      {items.map((s) => (
        <div
          key={s.target + s.name}
          className={"desktop-icon" + (selected === s.target ? " sel" : "")}
          title={s.target}
          onClick={() => setSelected(s.target)}
          onDoubleClick={() => launch(s)}
        >
          {s.icon ? (
            <img className="di-img" src={s.icon} alt="" draggable={false} />
          ) : (
            <div className="di-fallback">▣</div>
          )}
          <span className="di-label">{s.name}</span>
        </div>
      ))}
    </div>
  );
}
