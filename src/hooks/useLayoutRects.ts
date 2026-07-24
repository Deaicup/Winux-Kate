import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface SlotRectPayload {
  id: string;
  rect: Rect;
  kind: string;
  page: number;
}

/**
 * Measures every DOM element tagged with `data-slot` (relative to the webview,
 * converted to physical pixels) and reports their rectangles to the backend so
 * embedded external windows can be positioned over them. Re-runs on page change,
 * IM view change, and on resize.
 */
export function useLayoutRects(currentPage: number, imView: string) {
  useEffect(() => {
    const report = () => {
      const els = Array.from(
        document.querySelectorAll<HTMLElement>("[data-slot]")
      );
      if (els.length === 0) return;
      const dpr = window.devicePixelRatio || 1;
      const rects: SlotRectPayload[] = [];
      for (const el of els) {
        const br = el.getBoundingClientRect();
        // Skip elements inside hidden pages (display:none gives 0x0 rects).
        // Reporting zero rects would overwrite good rects in the backend.
        if (br.width === 0 || br.height === 0) continue;
        const kind = el.dataset.slotKind || el.dataset.slot || "terminal";
        rects.push({
          id: el.dataset.slot || "",
          kind,
          page: currentPage,
          rect: {
            x: Math.round(br.left * dpr),
            y: Math.round(br.top * dpr),
            w: Math.round(br.width * dpr),
            h: Math.round(br.height * dpr),
          },
        });
      }
      if (rects.length === 0) return;
      invoke("report_slot_rects", { rects }).catch(console.error);
    };

    // Report after layout settles, then observe resize.
    const t1 = setTimeout(report, 60);
    const t2 = setTimeout(report, 350);
    const ro = new ResizeObserver(() => report());
    document.querySelectorAll("[data-slot]").forEach((el) => {
      ro.observe(el);
    });
    const onResize = () => report();
    window.addEventListener("resize", onResize);
    window.addEventListener("wm-resize", onResize);

    return () => {
      clearTimeout(t1);
      clearTimeout(t2);
      ro.disconnect();
      window.removeEventListener("resize", onResize);
      window.removeEventListener("wm-resize", onResize);
    };
  }, [currentPage, imView]);
}
