import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useStore, type IdeInstance, type CustomPage, type AppDetection } from "../store";

interface IdeState {
  list: IdeInstance[];
  active: number;
}

async function refreshIde() {
  try {
    const st = await invoke<IdeState>("ide_list");
    useStore.getState().setIde(st.list, st.active);
  } catch (e) {
    console.error(e);
  }
}

async function refreshCustomPages() {
  try {
    const pages = await invoke<CustomPage[]>("list_custom_pages");
    useStore.getState().setCustomPages(pages);
  } catch (e) {
    console.error(e);
  }
}

async function refreshDetection() {
  try {
    const d = await invoke<AppDetection>("detect_apps");
    useStore.getState().setDetection(d);
  } catch (e) {
    console.error(e);
  }
}

/** Wires up all backend -> frontend events. Call once at the app root. */
export function useTauriEvents() {
  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];
    let cancelled = false;

    (async () => {
      unlisteners.push(
        await listen<number>("page-changed", (e) => {
          useStore.getState().setPage(e.payload);
        })
      );

      unlisteners.push(
        await listen<number>("ide-active-changed", () => {
          refreshIde();
        })
      );

      unlisteners.push(
        await listen<null>("ide-request-new", async () => {
          try {
            // Hide topmost IDE/IM overlays so the folder dialog is not covered.
            await invoke("hide_overlays");
            const folder = await open({ directory: true, multiple: false });
            const folderPath = typeof folder === "string" ? folder : null;
            await invoke("ide_new", { folder: folderPath });
            await refreshIde();
          } catch (e) {
            console.error(e);
          }
        })
      );

      unlisteners.push(
        await listen<"split" | "wecom">("im-toggle", (e) => {
          useStore.getState().setImView(e.payload);
        })
      );

      unlisteners.push(
        await listen("custom-pages-changed", () => {
          refreshCustomPages();
        })
      );

      unlisteners.push(
        await listen("wm-resize", () => {
          window.dispatchEvent(new CustomEvent("wm-resize"));
        })
      );

      if (!cancelled) {
        refreshIde();
        refreshCustomPages();
        refreshDetection();
      }
    })();

    return () => {
      cancelled = true;
      unlisteners.forEach((u) => u());
    };
  }, []);
}
