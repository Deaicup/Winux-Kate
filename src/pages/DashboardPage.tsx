import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Terminal } from "../components/Terminal";
import { CodeEditor } from "../components/CodeEditor";
import { FileViewer } from "../components/FileViewer";
import { StatusBar } from "../components/StatusBar";

export function DashboardPage() {
  const [path, setPath] = useState("");
  const [content, setContent] = useState("");

  const open = async (p: string) => {
    try {
      const text = await invoke<string>("read_file", { path: p });
      setPath(p);
      setContent(text);
    } catch (e) {
      alert(`打开失败: ${String(e)}`);
    }
  };

  const save = async () => {
    if (!path) return;
    try {
      await invoke("write_file", { path, content });
    } catch (e) {
      alert(`保存失败: ${String(e)}`);
    }
  };

  return (
    <div className="page dashboard">
      <div className="dash-grid">
        <div className="panel dash-term1">
          <div className="panel-header">
            <span className="dot" />
            TERM-01
          </div>
          <div className="panel-body">
            <Terminal cmd="powershell.exe" />
          </div>
        </div>
        <div className="panel dash-term2">
          <div className="panel-header">
            <span className="dot" />
            TERM-02
          </div>
          <div className="panel-body">
            <Terminal cmd="powershell.exe" />
          </div>
        </div>
        <div className="panel dash-edit">
          <div className="panel-header">
            <span className="dot" />
            EDITOR · {path || "untitled"}
            <button
              className="btn"
              style={{ marginLeft: "auto" }}
              onClick={save}
              disabled={!path}
            >
              SAVE
            </button>
          </div>
          <div className="panel-body">
            <CodeEditor path={path} content={content} onChange={setContent} />
          </div>
        </div>
        <div className="panel dash-files">
          <div className="panel-header">
            <span className="dot" />
            FILES
          </div>
          <div className="panel-body">
            <FileViewer onOpen={open} />
          </div>
        </div>
      </div>
      <StatusBar />
    </div>
  );
}
