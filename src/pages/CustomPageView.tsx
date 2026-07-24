import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useStore } from "../store";

interface CustomState {
  list: number[];
  active: number;
}

/** A user-defined page that embeds a single chosen application fullscreen,
 * with IDE-like multi-instance tabs (Ctrl+Shift+Tab cycles / creates). */
export function CustomPageView({ pageId }: { pageId: number }) {
  const [err, setErr] = useState<string | null>(null);
  const [st, setSt] = useState<CustomState>({ list: [], active: 0 });
  const page = useStore((s) => s.customPages.find((p) => p.id === pageId));
  const launchedRef = useRef(false);

  const refresh = () =>
    invoke<CustomState>("custom_state", { id: pageId })
      .then(setSt)
      .catch(() => {});

  useEffect(() => {
    if (launchedRef.current) return;
    launchedRef.current = true;
    setErr(null);
    invoke("launch_custom_page", { id: pageId })
      .then(() => refresh())
      .catch((e) => setErr(String(e)));
    const u = listen("custom-active-changed", (e) => {
      if (e.payload === pageId) refresh();
    });
    const id = setInterval(refresh, 1500);
    return () => {
      u.then((f) => f());
      clearInterval(id);
    };
  }, [pageId]);

  const newInst = () =>
    invoke("launch_custom_new", { id: pageId })
      .then(() => refresh())
      .catch((e) => setErr(String(e)));

  const selectInst = (i: number) =>
    invoke("set_custom_active", { id: pageId, index: i })
      .then(() => refresh())
      .catch(console.error);

  const closeInst = (i: number) =>
    invoke("close_custom", { id: pageId, index: i })
      .then(() => refresh())
      .catch(console.error);

  return (
    <div className="page ide">
      <div className="ide-tabs">
        <span className="dot" /> {page?.name ?? "自定义应用"}
        {st.list.map((h, i) => (
          <div
            key={h}
            className={"ide-tab" + (i === st.active ? " active" : "")}
            onClick={() => selectInst(i)}
          >
            {page?.name ?? "应用"} #{i + 1}
            <span
              className="pg-rm"
              title="关闭此窗口"
              onClick={(e) => {
                e.stopPropagation();
                closeInst(i);
              }}
            >
              ×
            </span>
          </div>
        ))}
        <button className="btn" onClick={newInst}>
          + 新建实例
        </button>
        <span className="ide-hint">Ctrl+Shift+Tab 切换 / 点击标签切换 / × 关闭</span>
      </div>
      <div className="ide-host" data-slot={`custom-${pageId}`} data-slot-kind="custom">
        {err ? (
          <div className="embed-empty">启动失败: {err}</div>
        ) : st.list.length === 0 ? (
          <div className="embed-empty">正在启动 {page?.name ?? "应用"}…</div>
        ) : null}
      </div>
    </div>
  );
}
