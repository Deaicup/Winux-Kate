import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface DirEntry {
  name: string;
  path: string;
  is_dir: boolean;
}

function parentOf(p: string): string | null {
  if (!p) return null;
  const idx = p.replace(/\/$/, "").lastIndexOf("\\");
  if (idx <= 0) return null;
  return p.slice(0, idx);
}

interface Props {
  onOpen: (path: string) => void;
}

export function FileViewer({ onOpen }: Props) {
  const [cwd, setCwd] = useState<string>("");
  const [entries, setEntries] = useState<DirEntry[]>([]);

  const load = (p?: string) => {
    invoke<DirEntry[]>("list_dir", { path: p ?? null })
      .then((es) => {
        setEntries(es);
        setCwd(p ?? "");
      })
      .catch((e) => console.error(e));
  };

  useEffect(() => {
    load();
  }, []);

  const open = (e: DirEntry) => {
    if (e.is_dir) load(e.path);
    else onOpen(e.path);
  };

  return (
    <div className="fileviewer">
      <div className="fv-path" title={cwd}>
        {cwd || "(home)"}
      </div>
      <div className="fv-list">
        {cwd && (
          <div className="fv-item dir" onClick={() => load(parentOf(cwd) ?? undefined)}>
            ▸ ../
          </div>
        )}
        {entries.map((e) => (
          <div
            key={e.path}
            className={"fv-item " + (e.is_dir ? "dir" : "file")}
            onDoubleClick={() => open(e)}
            title={e.path}
          >
            {e.is_dir ? "▸ " : "  "}
            {e.name}
          </div>
        ))}
      </div>
    </div>
  );
}
