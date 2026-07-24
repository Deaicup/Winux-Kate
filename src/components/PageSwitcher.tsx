import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useStore } from "../store";

const BUILTIN = [
  { n: 1, label: "DASHBOARD" },
  { n: 2, label: "IDE" },
  { n: 3, label: "IM" },
  { n: 4, label: "DESKTOP" },
];

export function PageSwitcher() {
  const current = useStore((s) => s.currentPage);
  const customPages = useStore((s) => s.customPages);

  const addPage = async () => {
    try {
      const exe = await open({
        multiple: false,
        filters: [{ name: "程序", extensions: ["exe"] }],
      });
      const exePath = typeof exe === "string" ? exe : null;
      if (!exePath) return;
      const name = window.prompt("页面名称", exePath.split(/[\\/]/).pop() || "应用");
      if (!name) return;
      await invoke("add_custom_page", { name, exe: exePath, args: "" });
    } catch (e) {
      alert(`添加失败: ${String(e)}`);
    }
  };

  const removePage = async (id: number) => {
    if (window.confirm("移除该自定义页？")) {
      await invoke("remove_custom_page", { id }).catch(console.error);
      // If the removed page was current, switch back to page 1.
      const current = useStore.getState().currentPage;
      if (current === id) {
        await invoke("set_current_page", { page: 1 });
      }
    }
  };

  return (
    <div className="page-switcher">
      {BUILTIN.map((p) => (
        <div
          key={p.n}
          className={"pg" + (current === p.n ? " active" : "")}
          onClick={() => invoke("set_current_page", { page: p.n })}
        >
          {p.n}·{p.label}
        </div>
      ))}
      {customPages.map((p) => (
        <div
          key={p.id}
          className={"pg custom" + (current === p.id ? " active" : "")}
          onClick={() => invoke("set_current_page", { page: p.id })}
          title={`${p.exe} ${p.args}`}
        >
          {p.id}·{p.name}
          <span
            className="pg-rm"
            onClick={(e) => {
              e.stopPropagation();
              removePage(p.id);
            }}
          >
            ×
          </span>
        </div>
      ))}
      <div className="pg pg-add" onClick={addPage} title="新增一个全屏应用页">
        +
      </div>
    </div>
  );
}
