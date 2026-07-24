import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useStore } from "../store";

export function IdePage() {
  const ideList = useStore((s) => s.ideList);
  const ideActive = useStore((s) => s.ideActive);
  const detection = useStore((s) => s.detection);

  const newIde = async () => {
    try {
      // Hide the topmost VSCode overlay so the folder dialog is not covered.
      await invoke("hide_overlays");
      const folder = await open({ directory: true, multiple: false });
      await invoke("ide_new", { folder: typeof folder === "string" ? folder : null });
    } catch (e) {
      alert(`启动 VSCode 失败: ${String(e)}`);
    }
  };

  if (detection && !detection.vscode) {
    return (
      <div className="page ide">
        <div className="ide-tabs">
        <span className="dot" /> IDE
        <span className="page-credit">由 Deaicup 工作室制作</span>
      </div>
        <div className="ide-host missing">
          <div className="missing-card">
            <h2>未检测到 VSCode</h2>
            <p>请安装 VSCode 后重启 Winux-Kate 以使用 IDE 页。</p>
            <a
              className="btn"
              href="https://code.visualstudio.com/"
              target="_blank"
              rel="noreferrer"
            >
              下载 VSCode
            </a>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="page ide">
      <div className="ide-tabs">
        {ideList.map((inst, i) => (
          <div
            key={inst.hwnd}
            className={"ide-tab" + (i === ideActive ? " active" : "")}
            onClick={() => invoke("ide_set_active", { index: i })}
            title={inst.folder ?? ""}
          >
            {inst.title}
          </div>
        ))}
        <button className="btn" onClick={newIde}>
          + NEW IDE
        </button>
        <span className="ide-hint">Ctrl+Shift+Tab 切换 / 末尾新建</span>
      </div>
      <div className="ide-host" data-slot="ide" data-slot-kind="ide">
        {ideList.length === 0 && (
          <div className="embed-empty">
            未启动 VSCode · 按 Ctrl+Shift+Tab 或点击 + NEW IDE
          </div>
        )}
      </div>
    </div>
  );
}
