import Editor from "@monaco-editor/react";
import { useEffect } from "react";

interface Props {
  path: string;
  content: string;
  onChange: (value: string) => void;
}

function languageFor(path: string): string {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  const map: Record<string, string> = {
    ts: "typescript", tsx: "typescript", js: "javascript", jsx: "javascript",
    json: "json", rs: "rust", py: "python", go: "go", md: "markdown",
    html: "html", css: "css", toml: "ini", yaml: "yaml", yml: "yaml",
    sh: "shell", ps1: "powershell", c: "c", cpp: "cpp", cs: "csharp",
  };
  return map[ext] ?? "plaintext";
}

export function CodeEditor({ path, content, onChange }: Props) {
  const language = languageFor(path);

  if (!path) {
    return (
      <div className="editor-empty">
        <span>// no file loaded - pick one from the file viewer</span>
      </div>
    );
  }

  return (
    <Editor
      height="100%"
      theme="vs-dark"
      language={language}
      value={content}
      onChange={(v) => onChange(v ?? "")}
      options={{
        fontFamily: "'JetBrains Mono', monospace",
        fontSize: 13,
        minimap: { enabled: false },
        scrollBeyondLastLine: false,
        automaticLayout: true,
      }}
    />
  );
}
