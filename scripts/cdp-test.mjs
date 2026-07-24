// CDP driver for Winux-Kate debug-build automated testing.
// Connects to the app's WebView2 via --remote-debugging-port=9223, performs
// clicks / page switches / instance management, captures webview + full-screen
// screenshots, and writes a results summary.
//
// Usage: node scripts/cdp-test.mjs   (from the project root, app already running)

import { writeFileSync, mkdirSync } from "node:fs";
import { execSync } from "node:child_process";

const CDP_HTTP = "http://127.0.0.1:9223";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let ws;
let msgId = 0;
const pending = new Map();

function send(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++msgId;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
  });
}

async function evaluate(expression) {
  const r = await send("Runtime.evaluate", {
    expression,
    returnByValue: true,
    awaitPromise: true,
  });
  if (r.exceptionDetails) {
    throw new Error("eval failed: " + JSON.stringify(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text));
  }
  return r.result?.value;
}

async function webviewShot(name) {
  const r = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(`shots/${name}.png`, Buffer.from(r.data, "base64"));
  console.log(`[shot] webview ${name}.png`);
}

function fullShot(name) {
  try {
    execSync(
      `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/capture-screen.ps1 -Out "shots/${name}.png"`,
      { stdio: "pipe" }
    );
    console.log(`[shot] fullscreen ${name}.png`);
  } catch (e) {
    console.log(`[shot] fullscreen ${name} FAILED: ${e.message}`);
  }
}

const results = [];
function record(name, ok, info = "") {
  results.push({ name, ok, info });
  console.log(`[test] ${ok ? "PASS" : "FAIL"} ${name}${info ? "  -- " + info : ""}`);
}

async function main() {
  mkdirSync("shots", { recursive: true });

  // ---- connect to CDP ----
  let targets = null;
  for (let i = 0; i < 60; i++) {
    try {
      const res = await fetch(`${CDP_HTTP}/json`);
      targets = await res.json();
      if (targets.some((t) => t.type === "page")) break;
    } catch {}
    await sleep(500);
  }
  if (!targets) throw new Error("CDP endpoint never came up");
  const page = targets.find((t) => t.type === "page");
  console.log(`[cdp] page: ${page.url}  title="${page.title}"`);

  ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((res, rej) => {
    ws.onopen = res;
    ws.onerror = () => rej(new Error("ws connect failed"));
  });
  ws.onmessage = (ev) => {
    const m = JSON.parse(ev.data);
    if (m.id && pending.has(m.id)) {
      const p = pending.get(m.id);
      pending.delete(m.id);
      m.error ? p.reject(new Error(m.error.message)) : p.resolve(m.result);
    }
  };
  await send("Page.enable");
  await send("Runtime.enable");

  // ---- wait for the boot screen to finish ----
  let booted = false;
  for (let i = 0; i < 40; i++) {
    const booting = await evaluate(`!!document.querySelector('.boot')`).catch(() => true);
    const ready = await evaluate(`!!document.querySelector('.page-switcher')`).catch(() => false);
    if (!booting && ready) { booted = true; break; }
    await sleep(500);
  }
  record("boot screen completes", booted);

  // ---- helpers ----
  const gotoPage = async (n) => {
    await evaluate(
      `[...document.querySelectorAll('.page-switcher .pg')].find(e => e.textContent.trim().startsWith('${n}\u00b7'))?.click()`
    );
    await sleep(1500);
  };
  const currentPage = () => evaluate(`window.__TAURI__.core.invoke('get_current_page')`);
  const customState = () => evaluate(`window.__TAURI__.core.invoke('custom_state', { id: 5 })`);

  // ---- 1. walk all pages 1..5 ----
  for (const n of [1, 2, 3, 4, 5]) {
    await gotoPage(n);
    const p = await currentPage();
    record(`goto page ${n}`, p === n, `backend page=${p}`);
    await webviewShot(`page${n}`);
  }
  await sleep(2000); // let launch_custom settle on page 5
  await fullShot("screen-page5");

  // ---- 2. custom page should have launched/adopted a Trae window ----
  let st = await customState();
  record("custom page has instance", st.list.length >= 1, JSON.stringify(st));

  // ---- 3. stay on page 5: verify Trae visible (full screen shot after fg settle) ----
  await sleep(2000);
  await fullShot("screen-page5-settled");
  await webviewShot("page5-tabs");

  // ---- 4. create a new instance via "+ 新建实例" ----
  const before = st.list.length;
  await evaluate(
    `[...document.querySelectorAll('.ide-tabs .btn')].find(b => b.textContent.includes('\u65b0\u5efa\u5b9e\u4f8b'))?.click()`
  );
  // launch_custom_new blocks up to 20s waiting for the window; poll state.
  for (let i = 0; i < 25; i++) {
    await sleep(1000);
    st = await customState();
    if (st.list.length > before) break;
  }
  record("new instance created", st.list.length > before, `before=${before} after=${st.list.length}`);
  await fullShot("screen-page5-newinst");

  // ---- 5. switch between instance tabs ----
  const tabCount = Math.min(st.list.length, 3);
  for (let i = 0; i < tabCount; i++) {
    await evaluate(`document.querySelectorAll('.ide-tab')[${i}]?.click()`);
    await sleep(2000);
    const s2 = await customState();
    record(`select instance #${i}`, s2.active === i, `active=${s2.active}/${s2.list.length}`);
    await fullShot(`screen-page5-inst${i}`);
  }

  // ---- 6. roundtrip: 5 -> 1 -> 5, window must come back to foreground ----
  await gotoPage(1);
  await fullShot("screen-page1");
  await gotoPage(5);
  const p5 = await currentPage();
  record("roundtrip back to page 5", p5 === 5, `backend page=${p5}`);
  await sleep(2000);
  await fullShot("screen-page5-roundtrip");

  // ---- 7. Ctrl+Tab hotkey via SendKeys (separate PowerShell, LL hook level) ----
  try {
    execSync(
      `powershell -NoProfile -Command "$ws = New-Object -ComObject wscript.shell; $ws.SendKeys('^{TAB}')"`,
      { stdio: "pipe" }
    );
    await sleep(1500);
    const p = await currentPage();
    record("Ctrl+Tab switches page", p === 1, `page after Ctrl+Tab=${p} (expect 1, wrap from 5)`);
  } catch (e) {
    record("Ctrl+Tab switches page", false, e.message);
  }

  // ---- summary ----
  writeFileSync("shots/results.json", JSON.stringify(results, null, 2));
  const fails = results.filter((r) => !r.ok).length;
  console.log(`\n[done] ${results.length - fails}/${results.length} passed, results -> shots/results.json`);
  process.exit(fails ? 1 : 0);
}

main().catch((e) => {
  console.error("[fatal]", e);
  process.exit(2);
});
