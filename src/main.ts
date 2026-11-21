import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save, confirm } from "@tauri-apps/plugin-dialog";
import "./styles.css";

window.addEventListener("error", (e) => invoke("ui_log", { msg: `main error: ${e.message} @ ${e.filename}:${e.lineno}` }).catch(() => {}));
window.addEventListener("unhandledrejection", (e) => invoke("ui_log", { msg: `main rejection: ${String(e.reason)}` }).catch(() => {}));

/* ================================================================ helpers */

type Any = any;

const esc = (s: unknown) =>
  String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]!));

function fmtBytes(b: number): string {
  if (!b) return "0 B";
  if (b >= 1e9) return (b / 1e9).toFixed(1) + " GB";
  if (b >= 1e6) return (b / 1e6).toFixed(0) + " MB";
  if (b >= 1e3) return (b / 1e3).toFixed(0) + " KB";
  return b + " B";
}

function ago(ts: number | null | undefined): string {
  if (!ts) return "never";
  const d = Date.now() / 1000 - ts;
  if (d < 90) return "just now";
  if (d < 3600) return Math.round(d / 60) + " min ago";
  if (d < 86400) return Math.round(d / 3600) + " h ago";
  return Math.round(d / 86400) + " days ago";
}

function uptime(s: number): string {
  const d = Math.floor(s / 86400), h = Math.floor((s % 86400) / 3600), m = Math.floor((s % 3600) / 60);
  return d ? `${d}d ${h}h` : h ? `${h}h ${m}m` : `${m}m`;
}

let snackTimer = 0;
function snack(msg: string, err = false) {
  const el = document.getElementById("snack")!;
  el.textContent = msg;
  el.className = "show" + (err ? " err" : "");
  clearTimeout(snackTimer);
  snackTimer = window.setTimeout(() => (el.className = ""), err ? 6000 : 3200);
}

/** Commands handled by the app itself; everything else runs in the elevated engine. */
const LOCAL = new Set(["ui_log", "autostart_get", "autostart_set", "quit_app", "toast_done", "toast_action", "tidy_preview", "tidy_run", "tidy_undo", "organize_preview", "organize_run", "organize_undo", "organize_set_kind",
  "storage_scan", "storage_recycle", "roblox_state", "roblox_apply", "roblox_restore", "monitors_list", "monitor_set",
  "look_export_file", "look_import_file", "cursor_set", "theme_apps_now", "taskbar_status"]);

function raw<T = Any>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return LOCAL.has(cmd) ? invoke<T>(cmd, args) : invoke<T>("engine", { cmd, args: args ?? {} });
}

async function call<T = Any>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await raw<T>(cmd, args);
  } catch (e) {
    snack(String(e), true);
    throw e;
  }
}

async function busy(btn: HTMLButtonElement, fn: () => Promise<unknown>) {
  const html = btn.innerHTML;
  btn.disabled = true;
  btn.innerHTML = `<i class="spin"></i>${html}`;
  try {
    await fn();
  } catch {
    /* already shown */
  } finally {
    btn.disabled = false;
    btn.innerHTML = html;
  }
}

const toggle = (attrs: string, on: boolean) =>
  `<label class="toggle"><input type="checkbox" ${attrs} ${on ? "checked" : ""}><i></i></label>`;

const loading = (text = "Loading") => `<div class="loading">${esc(text)}…</div>`;

function q<T extends Element = HTMLElement>(sel: string, root: ParentNode = document): T {
  return root.querySelector(sel) as T;
}
function qa<T extends Element = HTMLElement>(sel: string, root: ParentNode = document): T[] {
  return Array.from(root.querySelectorAll(sel)) as T[];
}

async function setCfg(patch: Record<string, unknown>) {
  await call("set_config", { patch });
}

/** Wire every `[data-cfg]` checkbox/select/input to the config. */
function bindCfg(root: ParentNode) {
  qa<HTMLInputElement>("[data-cfg]", root).forEach((el) => {
    el.addEventListener("change", async () => {
      const key = el.dataset.cfg!;
      let v: unknown = el.value;
      if (el.type === "checkbox") v = el.checked;
      else if (el.dataset.num !== undefined) v = Number(el.value);
      await setCfg({ [key]: v });
      snack("Saved");
    });
  });
}

/* ================================================================ icons */

const ico = (d: string) =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${d}</svg>`;
const ICONS: Record<string, string> = {
  home: ico('<path d="M3 11l9-7 9 7"/><path d="M5 10v10h14V10"/>'),
  progress: ico('<path d="M3 20h18"/><path d="M6 16l4-5 3 3 5-7"/><path d="M15 7h3v3"/>'),
  look: ico('<rect x="3" y="4" width="18" height="14" rx="2"/><path d="M3 15l5-5 4 4 3-3 6 6"/><circle cx="16" cy="8.5" r="1.5"/>'),
  speed: ico('<path d="M12 14l4-6"/><path d="M4.9 19a9 9 0 1 1 14.2 0"/>'),
  autopilot: ico('<path d="M12 2l3 7 7 .6-5.3 4.6L18.4 21 12 17.3 5.6 21l1.7-6.8L2 9.6 9 9z"/>'),
  games: ico('<rect x="2" y="7" width="20" height="11" rx="5"/><path d="M7 11v3M5.5 12.5h3"/><circle cx="16" cy="11.5" r="1"/><circle cx="18" cy="13.5" r="1"/>'),
  safety: ico('<path d="M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z"/><path d="M9 12l2 2 4-4"/>'),
  settings: ico('<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/>'),
};

/* ================================================================ shell */

type Page = { title: string; sub: string; render: (el: HTMLElement) => Promise<void> };
let timers: number[] = [];
let engineUp = false;
let current = "home";

document.getElementById("app")!.innerHTML = `
  <aside class="sidebar">
    <div class="brand"><img src="/moth-logo.png" alt=""><div class="word">NIGHT<span>MOTH</span></div></div>
    <nav class="nav" id="nav"></nav>
    <div class="status" id="status"><span class="dot"></span>Starting…</div>
  </aside>
  <section class="main">
    <header class="titlebar" data-tauri-drag-region>
      <h1 id="title" data-tauri-drag-region></h1>
      <div class="winbtns">
        <button id="min" title="Minimize">&#x2014;</button>
        <button id="close" class="close" title="Hide to tray">&#x2715;</button>
      </div>
    </header>
    <div class="content" id="content"></div>
  </section>
  <div id="snack"></div>`;

const win = getCurrentWindow();
q("#min").onclick = () => win.minimize();
q("#close").onclick = () => win.hide();

const PAGES: Record<string, Page> = {};
const NAV = ["home", "progress", "look", "speed", "autopilot", "games", "safety", "settings"];

function go(name: string) {
  timers.forEach(clearInterval);
  timers = [];
  current = name;
  qa("#nav button").forEach((b) => b.classList.toggle("active", b.dataset.page === name));
  const p = PAGES[name];
  q("#title").innerHTML = `${esc(p.title)}<small>${esc(p.sub)}</small>`;
  const content = q("#content");
  content.style.animation = "none";
  void content.offsetWidth;
  content.style.animation = "";
  content.innerHTML = loading(engineUp ? "Loading" : "Starting the Nightmoth engine");
  content.scrollTop = 0;
  p.render(content).catch((e) => (content.innerHTML = `<div class="empty">Something went wrong: ${esc(e)}</div>`));
}

async function refreshStatus() {
  try {
    const c = await raw<Any>("get_config");
    engineUp = true;
    const b = c.pending_boost;
    q("#status").innerHTML = b
      ? `<span class="dot on"></span>Boosting <b>${esc(b.game)}</b>`
      : c.autopilot
      ? `<span class="dot on"></span>Autopilot is on`
      : `<span class="dot"></span>Autopilot paused`;
  } catch {
    q("#status").innerHTML = `<span class="dot"></span>Engine starting…`;
  }
}
setInterval(refreshStatus, 5000);

/* ================================================================ home */

PAGES.home = {
  title: "Home",
  sub: "Your PC at a glance",
  async render(el) {
    const ov = await call<Any>("overview");
    const s = ov.stats;
    el.innerHTML = `
      <div class="hero">
        <div class="card" style="text-align:center">
          <div class="ring">
            <svg viewBox="0 0 200 200"><circle class="track" cx="100" cy="100" r="80" stroke-width="10" fill="none"/>
              <circle class="arc" id="arc" cx="100" cy="100" r="80" stroke-width="10" fill="none" stroke-linecap="round" stroke-dasharray="502.65" stroke-dashoffset="502.65"/></svg>
            <div class="center"><img src="/moth-logo.png" alt=""><b id="score">··</b><span>Health</span></div>
          </div>
          <button class="btn primary big" id="perfect" style="width:100%;justify-content:center">Make my PC perfect</button>
          <button class="btn" id="optimize" style="width:100%;justify-content:center;margin-top:8px">Quick optimize</button>
        </div>
        <div class="wallcard" style="background-image:url('${ov.wallpaper_thumb ?? ""}')">
          <div class="shade"></div>
          <div class="meta">
            <div>
              <div class="muted" style="font-size:12px;text-transform:uppercase;letter-spacing:.12em">Current look</div>
              <div style="font-size:20px;font-weight:700;margin-top:4px">${esc(ov.style ?? "Pick a wallpaper")}</div>
              <div class="muted" style="font-size:12.5px;margin-top:2px">${esc((ov.wallpaper || "").split("\\").pop())}</div>
            </div>
            <div class="row">${ov.accent ? `<div class="swatch" style="background:${esc(ov.accent)}"></div><span class="mono">${esc(ov.accent)}</span>` : ""}
              <button class="btn small" id="golook">Change</button></div>
          </div>
        </div>
      </div>

      <div class="section-title"><h2>Live</h2><span class="muted" id="procs"></span></div>
      <div class="grid g4">
        <div class="card stat"><div class="label">CPU</div><div class="value" id="cpu">–</div><div class="bar"><i id="cpub"></i></div></div>
        <div class="card stat"><div class="label">Memory</div><div class="value" id="ram">–</div><div class="bar"><i id="ramb"></i></div></div>
        <div class="card stat"><div class="label">System drive</div><div class="value" id="disk">–</div><div class="bar ok"><i id="diskb"></i></div></div>
        <div class="card stat"><div class="label">Uptime</div><div class="value" id="up">–</div><div class="muted" style="font-size:12px" id="top"></div></div>
      </div>

      <div class="grid g2" style="margin-top:14px">
        <div class="card"><h3>What needs attention</h3><p class="sub">Updated every 10 minutes</p><ul class="issues" id="issues">${loading("Checking")}</ul></div>
        <div class="card"><h3>Autopilot activity</h3><p class="sub">The latest things Nightmoth did on its own</p><div class="feed" id="feed">${loading()}</div></div>
      </div>
      <div class="grid g2" style="margin-top:14px">
        <div class="card"><h3>This week</h3><p class="sub">What Autopilot did for you</p>
          <div class="grid g2">
            <div class="stat"><div class="label">Freed</div><div class="value">${fmtBytes(s.freed_bytes)}</div></div>
            <div class="stat"><div class="label">Apps removed</div><div class="value">${s.apps_removed}</div></div>
            <div class="stat"><div class="label">Games boosted</div><div class="value">${s.games_boosted}</div></div>
            <div class="stat"><div class="label">Startup blocked</div><div class="value">${s.startup_blocked}</div></div>
          </div>
        </div>
      </div>`;

    q("#golook").onclick = () => go("look");
    q("#perfect").onclick = () => go("smart");
    const feed = async () => {
      const ch = await raw<Any[]>("changes_list").catch(() => []);
      const box = document.getElementById("feed");
      if (!box) return;
      const items = ch.slice(0, 7);
      box.innerHTML = items.length
        ? items.map((e) => `<div class="fitem"><span class="badge ${e.module === "Autopilot" ? "red" : ""}">${esc(e.module)}</span><div><b>${esc(e.title)}</b><span>${ago(e.ts)}${e.detail ? " · " + esc(e.detail) : ""}</span></div></div>`).join("")
        : `<div class="muted">Nothing yet — Autopilot acts when something needs doing.</div>`;
    };
    feed();
    timers.push(window.setInterval(feed, 15000));
    const opt = q<HTMLButtonElement>("#optimize");
    opt.onclick = () =>
      busy(opt, async () => {
        const r = await call<Any>("optimize_now");
        snack(`Freed ${fmtBytes(r.freed)} · ${r.privacy} privacy fixes · ${r.services} services off`);
        loadHealth(true);
      });

    const live = async () => {
      if (current !== "home") return;
      const l = await raw<Any>("live_stats").catch(() => null);
      if (!l || !q("#cpu")) return;
      const ramPct = (l.ram_used / l.ram_total) * 100;
      const diskPct = l.disk_total ? (l.disk_used / l.disk_total) * 100 : 0;
      q("#cpu").innerHTML = `${l.cpu.toFixed(0)}<small>%</small>`;
      q("#cpub").style.width = l.cpu + "%";
      q("#ram").innerHTML = `${(l.ram_used / 1e9).toFixed(1)}<small> / ${(l.ram_total / 1e9).toFixed(0)} GB</small>`;
      q("#ramb").style.width = ramPct + "%";
      q("#disk").innerHTML = `${diskPct.toFixed(0)}<small>% used</small>`;
      q("#diskb").style.width = diskPct + "%";
      q("#up").textContent = uptime(l.uptime);
      q("#procs").textContent = `${l.processes} processes`;
      q("#top").textContent = l.top.length ? `Busiest: ${l.top[0][0]}` : "";
    };
    live();
    timers.push(window.setInterval(live, 2000));

    async function loadHealth(refresh: boolean) {
      const h = await raw<Any>("health", { refresh }).catch(() => null);
      if (!h || !q("#arc")) return;
      q("#score").textContent = h.score;
      q("#arc").setAttribute("stroke-dashoffset", String(502.65 * (1 - h.score / 100)));
      q("#issues").innerHTML = h.issues.length
        ? h.issues.map((i: string) => `<li>${esc(i)}</li>`).join("")
        : `<li style="color:var(--text)">Nothing to fix — your PC is in great shape.</li>`;
    }
    loadHealth(false);
  },
};

/* ================================================================ make my PC perfect */

const SMART: [string, string, "engine" | "app"][] = [
  ["restore", "Making a restore point", "engine"],
  ["repair", "Repairing what other tools broke", "engine"],
  ["cleanup", "Clearing junk", "engine"],
  ["privacy", "Blocking ads & tracking in Windows", "engine"],
  ["services", "Turning off useless background services", "engine"],
  ["startup", "Making startup faster", "engine"],
  ["power", "Setting up Game Mode, updates and visuals", "engine"],
  ["dns", "Finding the fastest DNS", "engine"],
  ["automation", "Switching on Autopilot, widgets and the invisible taskbar", "engine"],
  ["organize", "Organizing your desktop apps", "app"],
  ["roblox", "Tuning Roblox for your PC", "app"],
];

let smartRunning = false;

PAGES.smart = {
  title: "Make my PC perfect",
  sub: "One click — every safe optimization, then Autopilot keeps it that way",
  async render(el) {
    el.innerHTML = `<div class="card panic"><div class="row between"><div><h3>Everything, done for you</h3>
        <p class="sub" style="margin:0">Repairs, cleanup, privacy, startup, network and your desktop — then everything stays automatic. Nothing is uninstalled here, and every change can be undone from Safety.</p></div>
        <button class="btn primary big" id="smartgo">${smartRunning ? "Working…" : "Start"}</button></div></div>
      <div class="card" style="margin-top:14px"><div class="steps" id="steps">${SMART.map(([id, label]) => `<div class="step" data-step="${id}"><i></i><div><b>${label}</b><span></span></div></div>`).join("")}</div></div>`;
    const go2 = q<HTMLButtonElement>("#smartgo");
    go2.disabled = smartRunning;
    go2.onclick = async () => {
      if (smartRunning) return;
      smartRunning = true;
      go2.disabled = true;
      go2.textContent = "Working…";
      let done = 0;
      for (const [id] of SMART) {
        const row = document.querySelector<HTMLElement>(`[data-step="${id}"]`);
        row?.classList.add("run");
        let line = "";
        try {
          if (id === "organize") { const n = await raw<number>("organize_run"); line = n ? `${n} apps sorted into folders` : "Already organized"; }
          else if (id === "roblox") { const r = await raw<Any>("roblox_apply"); line = `${r.fps} FPS cap · graphics ${r.quality}/10`; }
          else line = await raw<string>("smart_step", { step: id });
          done++;
          row?.classList.replace("run", "ok");
        } catch (e) {
          line = String(e).includes("isn't installed") ? "Roblox not installed — skipped" : String(e);
          row?.classList.replace("run", "skip");
        }
        const sp = row?.querySelector("span");
        if (sp) sp.textContent = line;
      }
      smartRunning = false;
      localStorage.setItem("nm-welcomed", "1");
      go2.disabled = false;
      go2.textContent = "Run again";
      snack(`Done — ${done} of ${SMART.length} steps. Autopilot takes it from here.`);
      refreshStatus();
    };
  },
};

/* ================================================================ progress */

function delta(before: number, after: number, lowerIsBetter: boolean, fmt: (n: number) => string): string {
  if (!before || before === after) return `<span class="muted">no change yet</span>`;
  const better = lowerIsBetter ? after < before : after > before;
  const pct = Math.abs(((after - before) / before) * 100);
  return `<span class="${better ? "ok" : "red"}">${better ? "▲" : "▼"} ${pct.toFixed(0)}% ${better ? "better" : "worse"}</span> <span class="muted">from ${fmt(before)}</span>`;
}

PAGES.progress = {
  title: "Progress",
  sub: "What Nightmoth changed — measured",
  async render(el) {
    const p = await call<Any>("progress");
    const a = p.first, b = p.latest;
    const secs = (ms: number) => (ms / 1000).toFixed(1) + " s";
    const tile = (label: string, value: string, d: string) => `<div class="card stat"><div class="label">${label}</div><div class="value">${value}</div><div style="font-size:12.5px">${d}</div></div>`;
    const since = a ? ago(a.t).replace(" ago", "") : "";
    const hist: Any[] = p.history || [];
    const max = Math.max(100, ...hist.map((h) => h.score));
    const w = p.week, lw = p.last_week;
    const vs = (now: number, prev: number, f: (n: number) => string = String) => `${f(now)} <small>${prev ? `vs ${f(prev)} last week` : ""}</small>`;
    el.innerHTML = `
      <div class="card panic"><div class="row between"><div><h3>Before vs now</h3>
        <p class="sub" style="margin:0">${a && b && a.t !== b.t ? `Measured while your PC was idle, first ${since} ago and again ${ago(b.t)}.` : "The first measurement was just taken — come back in a few days to see the difference."}</p></div>
        <button class="btn small" id="snap">Measure now</button></div></div>
      ${b ? `<div class="grid g4" style="margin-top:14px">
        ${tile("Health score", `${b.score}<small>/100</small>`, delta(a.score, b.score, false, String))}
        ${tile("Boot time", b.boot_ms ? secs(b.boot_ms) : "–", delta(a.boot_ms, b.boot_ms, true, secs))}
        ${tile("Startup apps", String(b.startup_apps), delta(a.startup_apps, b.startup_apps, true, String))}
        ${tile("Idle memory", `${(b.idle_ram_mb / 1024).toFixed(1)}<small> GB</small>`, delta(a.idle_ram_mb, b.idle_ram_mb, true, (n) => (n / 1024).toFixed(1) + " GB"))}
        ${tile("Idle CPU", `${b.idle_cpu.toFixed(0)}<small>%</small>`, delta(a.idle_cpu, b.idle_cpu, true, (n) => n.toFixed(0) + "%"))}
        ${tile("Junk waiting", fmtBytes(b.junk_bytes), delta(a.junk_bytes, b.junk_bytes, true, fmtBytes))}
        ${tile("Free space", fmtBytes(b.disk_free), delta(a.disk_free, b.disk_free, false, fmtBytes))}
        ${tile("Installed apps", String(b.apps), delta(a.apps, b.apps, true, String))}
      </div>` : ""}
      ${hist.length > 1 ? `<div class="section-title"><h2>Health over time</h2></div>
        <div class="card"><div class="chart">${hist.map((h, i) => `<div class="${i === hist.length - 1 ? "last" : ""}" style="height:${(h.score / max) * 100}%" title="${new Date(h.t * 1000).toLocaleDateString()}"><span>${h.score}</span></div>`).join("")}</div></div>` : ""}
      <div class="section-title"><h2>This week</h2><span class="muted">since ${new Date(p.week_start * 1000).toLocaleDateString()}</span></div>
      <div class="grid g4">
        <div class="card stat"><div class="label">Freed</div><div class="value">${vs(w.freed_bytes, lw.freed_bytes, fmtBytes)}</div></div>
        <div class="card stat"><div class="label">Apps removed</div><div class="value">${vs(w.apps_removed, lw.apps_removed)}</div></div>
        <div class="card stat"><div class="label">Games boosted</div><div class="value">${vs(w.games_boosted, lw.games_boosted)}</div></div>
        <div class="card stat"><div class="label">Startup blocked</div><div class="value">${vs(w.startup_blocked, lw.startup_blocked)}</div></div>
      </div>
      <p class="sub" style="margin-top:12px">All time: ${p.removed} unused apps removed · ${p.fixes} things repaired or reviewed from other tools.</p>`;
    const snap = q<HTMLButtonElement>("#snap");
    snap.onclick = () => busy(snap, async () => { await call("snapshot_now"); go("progress"); });
  },
};

/* ================================================================ look */

const IMG_EXT = ["jpg", "jpeg", "png", "bmp", "webp", "gif", "tif", "tiff"];

PAGES.look = {
  title: "Look",
  sub: "Pick a wallpaper — Nightmoth themes Windows around it",
  async render(el) {
    const st = await call<Any>("look_state");
    const c = st.config;
    const a = st.analysis;
    const d = st.desktop;
    const accent = st.accent || a?.accent || "#A3121A";
    const chosen = c.looks?.[st.wallpaper]?.accent_index ?? a?.accent_index;

    el.innerHTML = `
      <div class="look-top">
        <div class="preview" style="background-image:url('${st.thumb ?? ""}');--acc:${esc(accent)}">
          <div class="fake-taskbar" style="background:${a?.dark === false ? "rgba(240,240,240,.55)" : "rgba(10,10,12,.55)"}">
            <i class="accent"></i><i></i><i></i><i></i><i></i></div>
        </div>
        <div class="card">
          <div class="row between"><h3>Generated theme</h3>${a ? `<span class="badge red">${esc(a.style)}</span>` : ""}</div>
          ${a
            ? `<p class="sub">${a.dark ? "Dark" : "Light"} mode · brightness ${(a.brightness * 100).toFixed(0)}% · colour ${(a.saturation * 100).toFixed(0)}%<br>Calmest area: ${esc(a.empty_area)}</p>
               <div class="palette">${a.palette.map((p: string, i: number) => `<div title="${esc(p)}" class="${i === chosen ? "chosen" : ""}" style="background:${esc(p)}"></div>`).join("")}</div>
               <div class="row" style="margin-bottom:14px"><div class="swatch" style="background:${esc(accent)}"></div><span>Accent <span class="mono">${esc(accent)}</span></span></div>`
            : `<p class="sub">No wallpaper analysed yet.</p>`}
          <div class="row wrap">
            <button class="btn primary" id="pick">Choose photo</button>
            <button class="btn" id="shuffle" ${a ? "" : "disabled"}>Shuffle accent</button>
          </div>
          <div class="list" style="margin-top:12px">
            <div class="item"><div class="info"><div class="name">Theme Windows from wallpaper</div><div class="desc">Dark/light mode, accent, Start, taskbar, title bars</div></div>${toggle('data-cfg="auto_theme"', c.auto_theme)}</div>
            <div class="item"><div class="info"><div class="name">Match lock screen</div><div class="desc">Same picture on the lock screen</div></div>${toggle('data-cfg="lock_screen_sync"', c.lock_screen_sync)}</div>
            <div class="item"><div class="info"><div class="name">Lock my accent colour</div><div class="desc">Keep one colour for every wallpaper</div></div>
              <input type="color" id="lockcolor" value="${esc(c.lock_accent || accent)}">${toggle('id="lockon"', !!c.lock_accent)}</div>
          </div>
        </div>
      </div>

      <div class="section-title"><h2>Presets</h2></div>
      <div class="presets">
        <div class="preset" data-preset="void"><b>Void</b><span>Black-red wallpaper, invisible taskbar, empty desktop</span></div>
        <div class="preset" data-preset="neon"><b>Neon Night</b><span>Synthwave wallpaper, tinted glass taskbar, no icons</span></div>
        <div class="preset" data-preset="clean"><b>Clean</b><span>Keep your wallpaper, invisible taskbar, no clutter</span></div>
        <div class="preset" data-preset="default"><b>Windows default</b><span>Solid Windows taskbar and icons back (turns invisible taskbar off)</span></div>
      </div>

      <div class="section-title"><h2>Taskbar</h2><span class="muted">Stays put — only the bar itself changes</span></div>
      <div class="card"><div class="choice" id="tbstyle">
        ${[["clear", "Invisible", "No bar at all — just your app icons floating on the wallpaper"],
           ["tint", "Tinted glass", "A faint wash of your wallpaper's accent behind the icons"],
           ["normal", "Windows default", "The normal solid taskbar"]].map(([id, n, desc]) => `
          <div data-tb="${id}" class="${d.taskbar === id ? "on" : ""}"><div class="tbprev ${id}"><i></i><i></i><i></i><i></i></div><b>${n}</b><span>${desc}</span></div>`).join("")}
      </div><p class="sub" style="margin:12px 0 0">Invisible taskbar helper: <span id="tbstatus" class="mono">checking…</span></p></div>

      <div class="section-title"><h2>Desktop widgets</h2><span class="muted">Coloured and placed from your wallpaper</span></div>
      <div class="card"><div class="list">
        <div class="item"><div class="info"><div class="name">Show widgets on the desktop</div><div class="desc">They sit behind your windows, never take clicks and hide while you game</div></div>${toggle('data-cfg="widgets_on"', c.widgets_on)}</div>
        <div class="item"><div class="info"><div class="name">Widgets</div><div class="desc">Click to add or remove</div></div>
          <div class="chips" id="wkinds">${[["clock", "Clock"], ["calendar", "Calendar"], ["system", "System monitor"], ["media", "Now playing"], ["weather", "Weather"], ["notes", "Notes"], ["dock", "App dock"], ["week", "Nightmoth week"]].map(([id, n]) => `<button class="chip ${(c.widgets || []).includes(id) ? "on" : ""}" data-wk="${id}">${n}</button>`).join("")}</div></div>
        <div class="item"><div class="info"><div class="name">Where</div><div class="desc" id="spotdesc">${c.widget_spot === "auto" || !c.widget_spot ? `Automatic — the calmest part of your wallpaper${a ? ` (${esc(a.empty_area)})` : ""}` : esc(c.widget_spot)}</div></div>
          <button class="btn small ${c.widget_spot === "auto" ? "primary" : ""}" id="spotauto">Auto</button>
          <div class="spots" id="spots">${["top left", "top", "top right", "left", "center", "right", "bottom left", "bottom", "bottom right"].map((n, i) => `<button title="${n}" data-spot="${n}" class="${c.widget_spot === n ? "on" : c.widget_spot === "auto" && a && i === a.calm_cell ? "auto" : ""}"></button>`).join("")}</div></div>
        <div class="item"><div class="info"><div class="name">Style</div></div>
          <select data-cfg="widget_style">${[["glass", "Smoked glass"], ["solid", "Solid black"], ["minimal", "Minimal (text only)"]].map(([v, n]) => `<option value="${v}" ${c.widget_style === v ? "selected" : ""}>${n}</option>`).join("")}</select></div>
        <div class="item"><div class="info"><div class="name">Size</div></div><input type="range" min="0.8" max="1.4" step="0.05" id="wscale" value="${c.widget_scale ?? 1}"></div>
        <div class="item"><div class="info"><div class="name">Match wallpaper colour</div><div class="desc">Off = Nightmoth red</div></div>${toggle('data-cfg="widget_accent"', c.widget_accent)}</div>
        <div class="item"><div class="info"><div class="name">Weather for</div><div class="desc" id="wxdesc">${c.weather_city ? esc(c.weather_city) : "Type your city (weather from open-meteo.com, no account)"}</div></div>
          <input type="text" id="wxcity" placeholder="City" value="${esc(c.weather_city || "")}" style="width:160px"><button class="btn small" id="wxset">Set</button></div>
        <div class="item" style="align-items:flex-start"><div class="info"><div class="name">Notes</div><div class="desc">Shown on the Notes widget</div></div>
          <textarea id="notes" rows="3" style="width:300px;background:var(--panel-2);border:1px solid var(--border);border-radius:8px;padding:8px;color:var(--text);font:inherit;resize:vertical">${esc(c.notes || "")}</textarea></div>
        <div class="item"><div class="info"><div class="name">App dock</div><div class="desc" id="dockdesc">${(c.dock || []).length ? (c.dock as string[]).map((d) => esc(d.split("\\").pop())).join(" · ") : "No apps yet"}</div></div>
          <button class="btn small" id="dockadd">Add app</button>${(c.dock || []).length ? `<button class="btn small ghost" id="dockclear">Clear</button>` : ""}</div>
      </div></div>

      <div class="section-title"><h2>Beyond the wallpaper</h2></div>
      <div class="card"><div class="list">
        <div class="item"><div class="info"><div class="name">Theme my other apps</div><div class="desc">Windows Terminal and VS Code take the wallpaper's colours (originals backed up)</div></div>
          <button class="btn small ghost" id="themenow">Apply now</button>${toggle('data-cfg="theme_apps"', c.theme_apps)}</div>
        <div class="item"><div class="info"><div class="name">Accent-coloured mouse pointer</div><div class="desc">The arrow follows your accent colour</div></div>${toggle('id="cursoron"', c.accent_cursor)}</div>
        <div class="item"><div class="info"><div class="name">Accent-coloured folders</div><div class="desc">Folder icons in your accent colour (folders showing a picture preview keep it)</div></div>${toggle('id="folderson"', c.accent_folders)}</div>
      </div></div>

      <div class="section-title"><h2>Share looks</h2><span class="muted">Wallpaper, colours, taskbar and widgets in one file</span></div>
      <div class="card"><div class="row between"><p class="sub" style="margin:0">Send your setup to a friend, or keep a backup of a look you love.</p>
        <div class="row"><button class="btn" id="lexport">Export this look</button><button class="btn primary" id="limport">Import a look</button></div></div></div>

      <div class="section-title"><h2>Screens</h2><span class="muted">A different wallpaper on each screen</span></div>
      <div class="card" id="screens">${loading("Finding screens")}</div>

      <div class="section-title"><h2>Organize desktop</h2><span class="muted">Every app sorted into Games, Social, Browsers…</span></div>
      <div class="card" id="organize">${loading("Looking at every app on your desktop")}</div>

      <div class="section-title"><h2>Tidy desktop</h2><span class="muted">Loose files sorted into folders by type</span></div>
      <div class="card" id="tidy">${loading("Looking at your desktop")}</div>

      <div class="section-title"><h2>Desktop & taskbar</h2></div>
      <div class="card"><div class="list">
        <div class="item"><div class="info"><div class="name">Show desktop icons</div><div class="desc">Turn off for a perfectly clean wallpaper</div></div>${toggle('data-desk="icons"', d.icons)}</div>
        <div class="item"><div class="info"><div class="name">Centre taskbar icons</div><div class="desc">Off = left-aligned like Windows 10</div></div>${toggle('data-desk="center"', d.center)}</div>
        <div class="item"><div class="info"><div class="name">Task View button</div></div>${toggle('data-desk="taskview"', d.taskview)}</div>
        <div class="item"><div class="info"><div class="name">Widgets button</div><div class="desc">Weather/news panel on the taskbar</div></div>${toggle('data-desk="widgets"', d.widgets)}</div>
        <div class="item"><div class="info"><div class="name">Search on taskbar</div></div>
          <select id="search">${["Hidden", "Icon only", "Search box", "Icon and label"].map((n, i) => `<option value="${i}" ${i === d.search ? "selected" : ""}>${n}</option>`).join("")}</select></div>
      </div></div>

      <div class="section-title"><h2>Slideshow</h2></div>
      <div class="card"><div class="list">
        <div class="item"><div class="info"><div class="name">Rotate wallpapers from a folder</div><div class="desc" id="sfolder">${esc(c.slideshow_folder || "No folder chosen")}</div></div>
          <button class="btn small" id="pickfolder">Choose folder</button>
          <select id="smin">${[[15, "15 min"], [30, "30 min"], [60, "1 hour"], [180, "3 hours"], [1440, "Daily"]].map(([v, n]) => `<option value="${v}" ${v === c.slideshow_minutes ? "selected" : ""}>${n}</option>`).join("")}</select>
          ${toggle('id="son"', c.slideshow_on)}</div>
      </div><p class="sub" style="margin:10px 0 0">The whole theme changes with every wallpaper. Pauses while you're gaming.</p></div>

      <div class="section-title"><h2>Nightmoth Originals</h2><span class="muted">Free wallpapers made on your PC</span></div>
      <div class="gallery" id="gallery">${loading("Making wallpapers (first time only)")}</div>`;

    bindCfg(el);

    const apply = async (path: string) => {
      snack("Applying theme…");
      await call("look_apply", { path });
      go("look");
    };

    const pick = q<HTMLButtonElement>("#pick");
    pick.onclick = async () => {
      const p = await open({ multiple: false, directory: false, filters: [{ name: "Images", extensions: IMG_EXT }] });
      if (typeof p === "string") busy(pick, () => apply(p));
    };
    const sh = q<HTMLButtonElement>("#shuffle");
    sh.onclick = () => busy(sh, async () => { await call("look_shuffle"); go("look"); });

    const lockOn = q<HTMLInputElement>("#lockon");
    const lockColor = q<HTMLInputElement>("#lockcolor");
    const setLock = async () => {
      await call("look_lock_accent", { hex: lockOn.checked ? lockColor.value.toUpperCase() : null });
      go("look");
    };
    lockOn.onchange = setLock;
    lockColor.onchange = () => lockOn.checked && setLock();

    qa<HTMLElement>("[data-preset]").forEach((p) => {
      p.onclick = async () => {
        snack("Applying preset…");
        await call("preset_apply", { id: p.dataset.preset });
        go("look");
      };
    });

    qa<HTMLInputElement>("[data-desk]").forEach((t) => {
      t.onchange = async () => {
        await call("desktop_set", { key: t.dataset.desk, value: t.checked }).catch(() => (t.checked = !t.checked));
      };
    });
    const search = q<HTMLSelectElement>("#search");
    search.onchange = () => call("desktop_set", { key: "search", value: Number(search.value) });

    qa<HTMLElement>("[data-tb]").forEach((t) => {
      t.onclick = async () => {
        await call("desktop_set", { key: "taskbar", value: t.dataset.tb });
        qa("[data-tb]").forEach((x) => x.classList.toggle("on", x === t));
        snack(t.dataset.tb === "clear" ? "Taskbar is now invisible" : t.dataset.tb === "tint" ? "Tinted glass taskbar" : "Normal taskbar");
      };
    });

    const tbStatus = async () => {
      const t = await raw<string>("taskbar_status").catch(() => "");
      const e2 = document.getElementById("tbstatus");
      if (e2 && t) e2.textContent = t;
    };
    tbStatus();
    timers.push(window.setInterval(tbStatus, 3000));

    let kinds: string[] = [...(c.widgets || [])];
    qa<HTMLButtonElement>("[data-wk]").forEach((b) => {
      b.onclick = async () => {
        const k = b.dataset.wk!;
        kinds = kinds.includes(k) ? kinds.filter((x) => x !== k) : [...kinds, k];
        b.classList.toggle("on", kinds.includes(k));
        await setCfg({ widgets: kinds });
      };
    });
    const setSpot = async (spot: string) => {
      await setCfg({ widget_spot: spot });
      go("look");
    };
    q("#spotauto").onclick = () => setSpot("auto");
    qa<HTMLButtonElement>("[data-spot]").forEach((b) => (b.onclick = () => setSpot(b.dataset.spot!)));
    const wscale = q<HTMLInputElement>("#wscale");
    wscale.onchange = () => setCfg({ widget_scale: Number(wscale.value) });

    q("#wxset").onclick = async () => {
      const city = q<HTMLInputElement>("#wxcity").value.trim();
      if (!city) { await setCfg({ weather_city: "", weather_lat: 0, weather_lon: 0 }); q("#wxdesc").textContent = "No city"; return; }
      try {
        const r = await fetch(`https://geocoding-api.open-meteo.com/v1/search?count=1&name=${encodeURIComponent(city)}`).then((x) => x.json());
        const hit = r.results?.[0];
        if (!hit) { snack("Couldn't find that city", true); return; }
        const label = `${hit.name}${hit.country ? ", " + hit.country : ""}`;
        await setCfg({ weather_city: label, weather_lat: hit.latitude, weather_lon: hit.longitude });
        q("#wxdesc").textContent = label;
        if (!kinds.includes("weather")) { kinds = [...kinds, "weather"]; await setCfg({ widgets: kinds }); q('[data-wk="weather"]').classList.add("on"); }
        snack(`Weather for ${label}`);
      } catch { snack("No internet connection", true); }
    };
    const notes = q<HTMLTextAreaElement>("#notes");
    let noteTimer = 0;
    notes.oninput = () => { clearTimeout(noteTimer); noteTimer = window.setTimeout(() => setCfg({ notes: notes.value }), 600); };
    q("#dockadd").onclick = async () => {
      const f = await open({ multiple: false, filters: [{ name: "Apps", extensions: ["exe", "lnk", "url"] }] });
      if (typeof f !== "string") return;
      const dock = [...(c.dock || []), f].slice(0, 10);
      await setCfg({ dock });
      if (!kinds.includes("dock")) { kinds = [...kinds, "dock"]; await setCfg({ widgets: kinds }); }
      go("look");
    };
    const dc = document.getElementById("dockclear");
    if (dc) dc.onclick = async () => { await setCfg({ dock: [] }); go("look"); };
    q("#themenow").onclick = async () => { const n = await call<number>("theme_apps_now", { accent }); snack(n ? `Themed ${n} app${n > 1 ? "s" : ""}` : "Windows Terminal or VS Code not found"); };
    q<HTMLInputElement>("#cursoron").onchange = (e) => call("cursor_set", { on: (e.target as HTMLInputElement).checked, accent }).then(() => snack("Saved"));
    q<HTMLInputElement>("#folderson").onchange = (e) => call("folders_set", { on: (e.target as HTMLInputElement).checked }).then(() => snack("Saved"));
    q("#lexport").onclick = async () => {
      const path = await save({ defaultPath: "My look.nightmoth", filters: [{ name: "Nightmoth look", extensions: ["nightmoth"] }] });
      if (!path) return;
      await call("look_export_file", { path });
      snack("Look exported");
    };
    q("#limport").onclick = async () => {
      const f = await open({ multiple: false, filters: [{ name: "Nightmoth look", extensions: ["nightmoth", "json"] }] });
      if (typeof f !== "string") return;
      snack("Applying look…");
      const name = await call<string>("look_import_file", { path: f });
      snack(`"${name}" applied`);
      go("look");
    };
    raw<Any[]>("monitors_list").then((mons) => {
      const box = q("#screens");
      if (!box) return;
      if (mons.length < 2) { box.innerHTML = `<p class="sub" style="margin:0">Only one screen connected. Plug in another and each can have its own wallpaper.</p>`; return; }
      box.innerHTML = `<div class="list">${mons.map((m, i) => `<div class="item"><div class="info"><div class="name">Screen ${i + 1} ${m.primary ? `<span class="badge red">Main · drives the theme</span>` : ""}</div>
        <div class="desc">${m.w}×${m.h} · ${esc((m.wallpaper || "").split("\\").pop() || "no wallpaper")}</div></div><button class="btn small" data-mon="${esc(m.id)}" data-primary="${m.primary ? 1 : 0}">Choose wallpaper</button></div>`).join("")}</div>`;
      qa<HTMLButtonElement>("[data-mon]", box).forEach((b) => (b.onclick = async () => {
        const f = await open({ multiple: false, filters: [{ name: "Images", extensions: IMG_EXT }] });
        if (typeof f !== "string") return;
        await busy(b, () => call("monitor_set", { id: b.dataset.mon, path: f, primary: b.dataset.primary === "1" }));
        go("look");
      }));
    }).catch(() => { const box = q("#screens"); if (box) box.innerHTML = `<p class="sub" style="margin:0">Couldn't read your screens.</p>`; });

    const drawOrganize = async () => {
      const box = q("#organize");
      if (!box) return;
      const o = await raw<Any>("organize_preview").catch(() => null);
      if (!o) { box.innerHTML = `<div class="empty">Couldn't read the desktop.</div>`; return; }
      const apps: Any[] = o.apps;
      const loose = apps.filter((x) => !x.sorted).length;
      const groups = (o.kinds as string[]).map((k) => [k, apps.filter((x) => x.kind === k)] as [string, Any[]]).filter((g) => g[1].length);
      box.innerHTML = `<div class="row between"><div><h3>${apps.length ? `${apps.length} apps on your desktop${loose ? ` · ${loose} not sorted yet` : " · all sorted"}` : "No apps on your desktop"}</h3>
          <p class="sub" style="margin:0">Each app goes into a folder for its kind, worked out from its name, where it's installed, its publisher and more. Nothing is left over: anything unclear goes into "Other apps". Move an app between folders (here or yourself) and Nightmoth remembers.</p></div>
          <div class="row">${o.can_undo ? `<button class="btn" id="oundo">Undo</button>` : ""}<button class="btn primary" id="orun" ${loose ? "" : "disabled"}>Organize now</button></div></div>
        <div class="item" style="margin-top:10px;border-top:1px solid var(--border);padding-top:12px"><div class="info"><div class="name">Organize automatically</div><div class="desc">New apps get sorted within a minute of appearing on your desktop. Part of Autopilot.</div></div>${toggle('id="oauto"', c.organize_auto)}</div>
        ${groups.map(([k, list]) => `<div class="section-title" style="margin:16px 0 6px"><h2>${esc(k)}</h2><span class="muted">${list.length}</span></div>
          <div class="list">${list.map((x) => `<div class="item"><div class="info"><div class="name">${esc(x.name)} ${x.public ? `<span class="badge">All users</span>` : ""} ${x.sorted ? "" : `<span class="badge red">Not sorted</span>`}</div><div class="desc">${esc(x.why)}</div></div>
            <select data-okind="${esc(x.file)}">${(o.kinds as string[]).map((kk) => `<option ${kk === x.kind ? "selected" : ""}>${esc(kk)}</option>`).join("")}</select></div>`).join("")}</div>`).join("")}`;
      const run = document.getElementById("orun") as HTMLButtonElement | null;
      if (run) run.onclick = () => busy(run, async () => { const n = await call<number>("organize_run"); snack(`${n} apps sorted`); await drawOrganize(); });
      const un = document.getElementById("oundo") as HTMLButtonElement | null;
      if (un) un.onclick = () => busy(un, async () => { const n = await call<number>("organize_undo"); snack(`${n} apps put back`); await drawOrganize(); });
      q<HTMLInputElement>("#oauto").onchange = async (e) => {
        const on = (e.target as HTMLInputElement).checked;
        c.organize_auto = on;
        await setCfg({ organize_auto: on });
        if (on && loose) { const n = await call<number>("organize_run"); snack(`Organizing on · ${n} apps sorted`); await drawOrganize(); } else snack("Saved");
      };
      qa<HTMLSelectElement>("[data-okind]", box).forEach((sel) => (sel.onchange = async () => {
        await call("organize_set_kind", { file: sel.dataset.okind, kind: sel.value });
        snack(`Moved to ${sel.value} — remembered`);
        await drawOrganize();
      }));
    };
    drawOrganize();

    const drawTidy = async () => {
      const box = q("#tidy");
      if (!box) return;
      const t = await raw<Any>("tidy_preview").catch(() => null);
      if (!t) { box.innerHTML = `<div class="empty">Couldn't read the desktop.</div>`; return; }
      const total = t.groups.reduce((n: number, g: [string, number]) => n + g[1], 0);
      box.innerHTML = `<div class="row between"><div><h3>${total ? `${total} loose files on your desktop` : "Your desktop is tidy"}</h3>
          <p class="sub" style="margin:0">${total ? t.groups.map((g: [string, number]) => `${g[1]} → ${esc(g[0])}`).join(" · ") : "Nothing to sort."}${t.last ? ` · last tidy ${ago(t.last)}` : ""}</p></div>
          <div class="row">${t.can_undo ? `<button class="btn" id="tundo">Undo last tidy</button>` : ""}<button class="btn primary" id="trun" ${total ? "" : "disabled"}>Tidy now</button></div></div>
        <div class="item" style="margin-top:10px;border-top:1px solid var(--border);padding-top:12px"><div class="info"><div class="name">Tidy automatically every day</div><div class="desc">Only files you haven't touched for a day. Part of Autopilot.</div></div>${toggle('id="tauto"', c.tidy_auto)}</div>`;
      const run = q<HTMLButtonElement>("#trun");
      run.onclick = () => busy(run, async () => { const n = await call<number>("tidy_run"); snack(`${n} files sorted`); await drawTidy(); });
      const undo = document.getElementById("tundo") as HTMLButtonElement | null;
      if (undo) undo.onclick = () => busy(undo, async () => { const n = await call<number>("tidy_undo"); snack(`${n} files put back`); await drawTidy(); });
      const ta = q<HTMLInputElement>("#tauto");
      ta.onchange = async () => { c.tidy_auto = ta.checked; await setCfg({ tidy_auto: ta.checked }); snack("Saved"); };
    };
    drawTidy();

    let folder = c.slideshow_folder as string;
    const son = q<HTMLInputElement>("#son");
    const smin = q<HTMLSelectElement>("#smin");
    const saveShow = async () => {
      try {
        const n = await call<number>("slideshow_set", { on: son.checked, folder, minutes: Number(smin.value) });
        if (son.checked) snack(`Slideshow on · ${n} images`);
      } catch {
        son.checked = false;
      }
    };
    q("#pickfolder").onclick = async () => {
      const p = await open({ directory: true, multiple: false });
      if (typeof p === "string") {
        folder = p;
        q("#sfolder").textContent = p;
        son.checked = true;
        saveShow();
      }
    };
    son.onchange = saveShow;
    smin.onchange = saveShow;

    raw<Any[]>("gallery_list")
      .then((items) => {
        const g = q("#gallery");
        if (!g) return;
        g.innerHTML = items
          .map((i) => `<div class="tile" data-path="${esc(i.path)}"><img src="${i.thumb}" alt=""><span>${esc(i.name)}</span></div>`)
          .join("");
        qa<HTMLElement>(".tile", g).forEach((t) => (t.onclick = () => apply(t.dataset.path!)));
      })
      .catch((e) => {
        const g = q("#gallery");
        if (g) g.innerHTML = `<div class="empty">${esc(e)}</div>`;
      });
  },
};

/* ================================================================ speed */

let speedTab = "startup";

PAGES.speed = {
  title: "Speed",
  sub: "Only tweaks that actually make a difference",
  async render(el) {
    const tabs: [string, string][] = [
      ["startup", "Startup"], ["cleanup", "Cleanup"], ["deep", "Deep clean"], ["storage", "Storage"], ["repair", "Repair"],
      ["updates", "App updates"], ["network", "Network"], ["debloat", "Debloat"], ["services", "Services"],
      ["privacy", "Privacy & ads"], ["power", "Power"], ["boot", "Boot time"],
    ];
    el.innerHTML = `<div class="seg" id="seg">${tabs.map(([id, n]) => `<button data-t="${id}" class="${id === speedTab ? "active" : ""}">${n}</button>`).join("")}</div><div id="sp"></div>`;
    qa<HTMLButtonElement>("#seg button").forEach((b) => (b.onclick = () => { speedTab = b.dataset.t!; go("speed"); }));
    const sp = q("#sp");
    sp.innerHTML = loading();
    await SPEED[speedTab](sp);
  },
};

const SPEED: Record<string, (el: HTMLElement) => Promise<void>> = {
  async startup(el) {
    const items = await call<Any[]>("startup_list");
    const c = await call<Any>("get_config");
    const delayed: Any[] = c.delayed || [];
    const isDelayed = (id: string) => delayed.some((d) => d.id === id);
    const on = items.filter((i) => i.enabled).length;
    el.innerHTML = `<div class="card"><div class="row between"><div><h3>Apps that start with Windows</h3>
      <p class="sub">${on} of ${items.length} enabled. Turning things off is the #1 way to boot faster. <b>Delay</b> keeps an app but starts it 60 s after login, so your desktop is ready first. Autopilot blocks new ones automatically.</p></div></div>
      <div class="list">${items.map((i) => `
        <div class="item"><div class="info"><div class="name">${esc(i.name)}</div><div class="desc" title="${esc(i.command)}">${esc(i.location)} · ${esc(i.command)}</div></div>
          <span class="badge ${i.impact === "High" ? "red" : ""}">${esc(i.impact)}</span>
          ${i.id.startsWith("task:") ? "" : isDelayed(i.id) ? `<button class="btn small" data-undelay="${esc(i.id)}">Delayed · undo</button>` : `<button class="btn small ghost" data-delay="${esc(i.id)}">Delay</button>`}
          ${isDelayed(i.id) ? "" : toggle(`data-id="${esc(i.id)}"`, i.enabled)}</div>`).join("") || `<div class="empty">Nothing starts with Windows. Nice.</div>`}
      </div></div>`;
    qa<HTMLInputElement>("[data-id]", el).forEach((t) => {
      t.onchange = () => call("startup_set", { id: t.dataset.id, enabled: t.checked }).then(() => snack("Saved")).catch(() => (t.checked = !t.checked));
    });
    qa<HTMLButtonElement>("[data-delay]", el).forEach((b) => (b.onclick = () => busy(b, async () => { await call("startup_delay", { id: b.dataset.delay, on: true }); snack("Starts 60 s after login now"); await SPEED.startup(el); })));
    qa<HTMLButtonElement>("[data-undelay]", el).forEach((b) => (b.onclick = () => busy(b, async () => { await call("startup_delay", { id: b.dataset.undelay, on: false }); snack("Back to normal"); await SPEED.startup(el); })));
  },

  async deep(el) {
    const d = await call<Any>("deep_scan");
    const total = d.items.reduce((n: number, i: Any) => n + (i.bytes || 0), 0);
    el.innerHTML = `<div class="card"><div class="row between"><div><h3>Deep clean${total ? ` · ${fmtBytes(total)}+ found` : ""}</h3>
      <p class="sub">The big stuff normal cleaners skip. Deleted straight away. Autopilot runs this by itself when you're away${d.last ? ` · last run ${ago(d.last)}` : ""}.</p></div>
      <button class="btn primary" id="drun">Deep clean selected</button></div>
      <div class="list">${d.items.map((i: Any) => `
        <div class="item"><input type="checkbox" data-d="${i.id}" checked style="accent-color:var(--red)">
          <div class="info"><div class="name">${esc(i.name)}</div><div class="desc">${esc(i.desc)}</div></div>
          <span class="mono ${i.bytes > 1e9 ? "red" : "muted"}">${i.bytes ? fmtBytes(i.bytes) : "—"}</span></div>`).join("")}</div>
      ${d.report?.length ? `<p class="sub" style="margin:12px 0 0">Last time: ${d.report.filter((r: Any) => r[1] > 0).map((r: Any) => `${esc(r[0])} ${fmtBytes(r[1])}`).join(" · ") || "nothing to remove"}</p>` : ""}</div>`;
    const btn = q<HTMLButtonElement>("#drun", el);
    btn.onclick = () => busy(btn, async () => {
      const ids = qa<HTMLInputElement>("[data-d]", el).filter((x) => x.checked).map((x) => x.dataset.d);
      snack("Deep cleaning — the Windows components step can take a few minutes…");
      const r = await call<Any>("deep_run", { ids });
      snack(`Freed ${fmtBytes(r.total)}`);
      await SPEED.deep(el);
    });
  },

  async storage(el) {
    el.innerHTML = `<div class="card"><div class="row between"><div><h3>What's using your space</h3>
      <p class="sub">Biggest files, duplicates and old downloads in your own folders. Deleting sends them to the Recycle Bin.</p></div>
      <button class="btn primary" id="sscan">Scan my folders</button></div><div id="sres"></div></div>`;
    const scan = q<HTMLButtonElement>("#sscan", el);
    const row = (f: Any, extra = "") => `<div class="item"><input type="checkbox" data-f="${esc(f.path)}" style="accent-color:var(--red)">
      <div class="info"><div class="name">${esc(f.name)}</div><div class="desc" title="${esc(f.path)}">${esc(f.path)}${extra}</div></div><span class="mono">${fmtBytes(f.size)}</span></div>`;
    scan.onclick = () => busy(scan, async () => {
      const r = await call<Any>("storage_scan");
      const dupSave = r.duplicates.reduce((n: number, g: Any[]) => n + g[0].size * (g.length - 1), 0);
      q("#sres", el).innerHTML = `<p class="sub" style="margin:10px 0">${r.scanned.toLocaleString()} files checked in ${r.seconds.toFixed(0)} s.</p>
        <div class="section-title"><h2>Biggest files</h2></div><div class="list">${r.large.map((f: Any) => row(f, ` · ${ago(f.modified)}`)).join("") || `<div class="empty">No files over 500 MB.</div>`}</div>
        <div class="section-title"><h2>Duplicates</h2><span class="muted">${fmtBytes(dupSave)} wasted · the oldest copy is kept unticked</span></div>
        <div class="list">${r.duplicates.map((g: Any[]) => g.map((f: Any, i: number) => i === 0 ? `<div class="item"><span class="badge white">Keep</span><div class="info"><div class="name">${esc(f.name)}</div><div class="desc">${esc(f.path)}</div></div><span class="mono">${fmtBytes(f.size)}</span></div>` : row(f, " · copy")).join("")).join("") || `<div class="empty">No duplicates found.</div>`}</div>
        <div class="section-title"><h2>Downloads untouched for 3+ months</h2></div><div class="list">${r.old_downloads.map((f: Any) => row(f, ` · ${ago(f.modified)}`)).join("") || `<div class="empty">Nothing old in Downloads.</div>`}</div>
        <div class="row" style="margin-top:12px"><button class="btn primary" id="srm">Move selected to Recycle Bin</button></div>`;
      const rm = q<HTMLButtonElement>("#srm", el);
      rm.onclick = () => busy(rm, async () => {
        const paths = qa<HTMLInputElement>("[data-f]", el).filter((x) => x.checked).map((x) => x.dataset.f);
        if (!paths.length) return;
        const n = await call<number>("storage_recycle", { paths });
        snack(`${n} files moved to the Recycle Bin`);
        qa<HTMLInputElement>("[data-f]", el).filter((x) => x.checked).forEach((x) => x.closest(".item")?.remove());
      });
    });
  },

  async repair(el) {
    const r = await call<Any>("fixer_scan");
    const bad = r.found.filter((f: Any) => f.kind === "fix");
    const good = r.found.filter((f: Any) => f.kind === "keep");
    const hist: Any[] = (r.history || []).filter((h: Any) => h.action === "fixed").slice().reverse();
    el.innerHTML = `<div class="card ${bad.length ? "panic" : ""}"><div class="row between"><div><h3>${bad.length ? `${bad.length} things another tool broke` : "Nothing broken by other tools"}</h3>
      <p class="sub" style="margin:0">Other optimizers and debloat scripts often switch off Windows Update, Defender or core services, or add "tweaks" that cause stutter. Nightmoth puts the harmful ones back automatically and keeps the good ones. Everything can be undone from Safety.</p></div>
      ${bad.length ? `<button class="btn primary" id="fixall">Fix all</button>` : ""}</div>
      ${bad.length ? `<div class="list" style="margin-top:10px">${bad.map((f: Any) => `<div class="item"><span class="badge red">Fix</span><div class="info"><div class="name">${esc(f.title)}</div><div class="desc">${esc(f.detail)}${f.restart ? " · needs a restart" : ""}</div></div></div>`).join("")}</div>` : ""}</div>
      ${good.length ? `<div class="section-title"><h2>Good tweaks kept</h2></div><div class="card"><div class="list">${good.map((f: Any) => `<div class="item"><span class="badge white">Kept</span><div class="info"><div class="name">${esc(f.title)}</div><div class="desc">${esc(f.detail)}</div></div></div>`).join("")}</div></div>` : ""}
      <div class="section-title"><h2>Repaired before</h2></div><div class="card"><div class="list">${hist.map((h: Any) => `<div class="item"><div class="info"><div class="name">${esc(h.title)}</div><div class="desc">${ago(h.t)} · ${esc(h.detail)}</div></div></div>`).join("") || `<div class="empty">Nothing yet.</div>`}</div></div>`;
    const fa = document.getElementById("fixall") as HTMLButtonElement | null;
    if (fa) fa.onclick = () => busy(fa, async () => {
      const x = await call<Any>("fixer_fix");
      snack(`${x.fixed} repaired${x.restart ? " · restart to finish" : ""}`, x.errors?.length > 0);
      await SPEED.repair(el);
    });
  },

  async updates(el) {
    el.innerHTML = `<div class="card">${loading("Asking winget for updates")}</div>`;
    const list = await call<Any[]>("updates_list");
    el.innerHTML = `<div class="card"><div class="row between"><div><h3>${list.length ? `${list.length} apps can be updated` : "Everything is up to date"}</h3>
      <p class="sub" style="margin:0">Through winget, Windows' own package manager. Autopilot installs updates by itself when you're away and plugged in.</p></div>
      ${list.length ? `<button class="btn primary" id="uall">Update all</button>` : ""}</div>
      <div class="list" style="margin-top:8px">${list.map((u) => `<div class="item"><div class="info"><div class="name">${esc(u.name)}</div><div class="desc mono">${esc(u.id)} · ${esc(u.version)} → ${esc(u.available)}</div></div><button class="btn small" data-up="${esc(u.id)}">Update</button></div>`).join("")}</div></div>`;
    const all = document.getElementById("uall") as HTMLButtonElement | null;
    if (all) all.onclick = () => busy(all, async () => { await call("updates_run", {}); await SPEED.updates(el); });
    qa<HTMLButtonElement>("[data-up]", el).forEach((b) => (b.onclick = () => busy(b, async () => { await call("updates_run", { id: b.dataset.up }); b.closest(".item")?.remove(); })));
  },

  async network(el) {
    const c = await call<Any>("get_config");
    el.innerHTML = `<div class="grid g2">
      <div class="card"><div class="row between"><h3>Fastest DNS</h3><button class="btn primary" id="dnsgo">Test now</button></div>
        <p class="sub">DNS turns names like roblox.com into addresses. A faster one makes every site and game lobby connect quicker. Tested from your PC; switching can be undone from Safety.</p>
        <div class="list" id="dnsres"></div>
        <div class="item" style="border-top:1px solid var(--border);margin-top:6px"><div class="info"><div class="name">Pick the fastest automatically</div><div class="desc">Re-tested every two weeks; only switches when another is clearly faster</div></div>${toggle('data-cfg="dns_auto"', c.dns_auto)}</div></div>
      <div class="card"><h3>Ping</h3><p class="sub">Round trip to Cloudflare, Google and a game server region.</p>
        <div class="grid g3" id="pings">${["1.1.1.1", "8.8.8.8", "9.9.9.9"].map((ip) => `<div class="stat"><div class="label">${ip}</div><div class="value" data-ping="${ip}">–</div></div>`).join("")}</div></div>
    </div>`;
    bindCfg(el);
    const go = q<HTMLButtonElement>("#dnsgo", el);
    go.onclick = () => busy(go, async () => {
      const res = await call<Any[]>("dns_bench");
      const best = res.find((r) => !r.failed);
      q("#dnsres", el).innerHTML = res.map((r) => `<div class="item"><div class="info"><div class="name">${esc(r.name)} ${r.current ? `<span class="badge white">In use</span>` : ""} ${r === best ? `<span class="badge red">Fastest</span>` : ""}</div>
        <div class="desc mono">${esc(r.primary)} ${esc(r.secondary)}${r.failed ? ` · ${r.failed} lookups failed` : ""}</div></div><span class="mono">${r.ms.toFixed(0)} ms</span>
        ${r.current ? "" : `<button class="btn small" data-dns="${esc(r.name)}|${esc(r.primary)}|${esc(r.secondary)}">Use</button>`}</div>`).join("");
      qa<HTMLButtonElement>("[data-dns]", el).forEach((b) => (b.onclick = () => busy(b, async () => {
        const [name, primary, secondary] = b.dataset.dns!.split("|");
        await call("dns_set", { name, primary, secondary });
        snack(`Using ${name}`);
      })));
    });
    const pings = async () => {
      for (const el2 of qa<HTMLElement>("[data-ping]", el)) {
        const ms = await raw<number | null>("net_ping", { ip: el2.dataset.ping }).catch(() => null);
        el2.innerHTML = ms == null ? `<small>no reply</small>` : `${ms}<small> ms</small>`;
      }
    };
    pings();
    timers.push(window.setInterval(pings, 4000));
  },

  async cleanup(el) {
    const targets = await call<Any[]>("cleanup_scan");
    const total = targets.reduce((s, t) => s + t.bytes, 0);
    el.innerHTML = `<div class="card"><div class="row between"><div><h3>${fmtBytes(total)} of junk found</h3>
      <p class="sub">Safe to delete. Browser caches are skipped while the browser is open.</p></div>
      <button class="btn primary" id="clean">Clean selected</button></div>
      <div class="list">${targets.map((t) => `
        <div class="item"><input type="checkbox" data-c="${t.id}" ${t.bytes > 0 && t.id !== "recycle" ? "checked" : ""} style="accent-color:var(--red)">
          <div class="info"><div class="name">${esc(t.name)}</div><div class="desc">${esc(t.desc)}</div></div>
          <span class="mono ${t.bytes > 1e8 ? "red" : "muted"}">${fmtBytes(t.bytes)}</span></div>`).join("")}</div></div>`;
    const btn = q<HTMLButtonElement>("#clean", el);
    btn.onclick = () => busy(btn, async () => {
      const ids = qa<HTMLInputElement>("[data-c]", el).filter((c) => c.checked).map((c) => c.dataset.c);
      const freed = await call<number>("cleanup_run", { ids });
      snack(`Freed ${fmtBytes(freed)}`);
      await SPEED.cleanup(el);
    });
  },

  async debloat(el) {
    const items = await call<Any[]>("debloat_list");
    const inst = items.filter((i) => i.installed);
    el.innerHTML = `<div class="card"><div class="row between"><div><h3>Preinstalled apps</h3>
      <p class="sub">${inst.length} found on this PC. A restore point is made before removing. You can get any of them back from the Microsoft Store.</p></div>
      <button class="btn primary" id="rm" ${inst.length ? "" : "disabled"}>Remove selected</button></div>
      <div class="list">${items.map((i) => `
        <div class="item" style="${i.installed ? "" : "opacity:.4"}"><input type="checkbox" data-b="${esc(i.id)}" ${i.installed ? "checked" : "disabled"} style="accent-color:var(--red)">
          <div class="info"><div class="name">${esc(i.name)}</div><div class="desc">${esc(i.desc)}</div></div>
          <span class="badge">${i.installed ? "Installed" : "Not installed"}</span></div>`).join("")}</div></div>`;
    const btn = q<HTMLButtonElement>("#rm", el);
    btn.onclick = async () => {
      const ids = qa<HTMLInputElement>("[data-b]", el).filter((c) => c.checked && !c.disabled).map((c) => c.dataset.b);
      if (!ids.length) return;
      if (!(await confirm(`Remove ${ids.length} preinstalled apps?`, { title: "Nightmoth", kind: "warning" }))) return;
      busy(btn, async () => {
        const n = await call<number>("debloat_remove", { ids });
        snack(`${n} apps removed`);
        await SPEED.debloat(el);
      });
    };
  },

  async services(el) {
    const items = await call<Any[]>("services_list");
    el.innerHTML = `<div class="card"><h3>Background services</h3>
      <p class="sub">Only services on Nightmoth's safe list. Toggle on = turned off. Xbox ones only matter if you don't use Xbox or Game Pass.</p>
      <div class="list">${items.map((s) => `
        <div class="item" style="${s.exists ? "" : "opacity:.4"}"><div class="info"><div class="name">${esc(s.display)}</div><div class="desc">${esc(s.desc)}</div></div>
          <span class="badge ${s.group === "telemetry" ? "red" : ""}">${esc(s.group)}</span>
          ${s.exists ? toggle(`data-s="${esc(s.name)}"`, s.start === 4) : `<span class="muted">Not on this PC</span>`}</div>`).join("")}</div></div>`;
    qa<HTMLInputElement>("[data-s]", el).forEach((t) => {
      t.onchange = () => call("service_set", { name: t.dataset.s, disabled: t.checked }).then(() => snack(t.checked ? "Service turned off" : "Service back on")).catch(() => (t.checked = !t.checked));
    });
  },

  async privacy(el) {
    const items = await call<Any[]>("privacy_list");
    el.innerHTML = `<div class="card"><div class="row between"><div><h3>Ads & tracking in Windows</h3>
      <p class="sub">Toggle on = blocked. Everything is reversible from Safety.</p></div><button class="btn" id="all">Block all</button></div>
      <div class="list">${items.map((p) => `
        <div class="item"><div class="info"><div class="name">${esc(p.name)}</div><div class="desc">${esc(p.desc)}</div></div>${toggle(`data-p="${p.id}"`, p.on)}</div>`).join("")}</div></div>`;
    qa<HTMLInputElement>("[data-p]", el).forEach((t) => {
      t.onchange = () => call("privacy_set", { id: t.dataset.p, on: t.checked }).then(() => snack("Saved")).catch(() => (t.checked = !t.checked));
    });
    const all = q<HTMLButtonElement>("#all", el);
    all.onclick = () => busy(all, async () => {
      for (const p of items.filter((p) => !p.on)) await call("privacy_set", { id: p.id, on: true });
      await SPEED.privacy(el);
    });
  },

  async power(el) {
    const p = await call<Any>("power_state");
    const v = await call<Any>("visuals_state");
    const pw = await call<Any>("power_info");
    const c = await call<Any>("get_config");
    el.innerHTML = `<div class="grid g2">
      <div class="card"><h3>Power plan</h3><p class="sub">Ultimate Performance keeps the CPU at full speed. Game boosts switch to it automatically.</p>
        <div class="list">${p.plans.map((pl: Any) => `
          <div class="item"><div class="info"><div class="name">${esc(pl.name || pl.guid)}</div><div class="desc mono">${esc(pl.guid)}</div></div>
          ${pl.active ? `<span class="badge red">Active</span>` : `<button class="btn small" data-plan="${pl.guid}">Use</button>`}</div>`).join("")}</div>
        <button class="btn primary" id="ult" style="margin-top:12px">Use Ultimate Performance</button></div>
      <div class="card"><h3>Gaming</h3><p class="sub">Windows' own performance switches.</p><div class="list">
        <div class="item"><div class="info"><div class="name">Windows Game Mode</div><div class="desc">Prioritises games, pauses Windows Update installs while playing</div></div>${toggle('id="gm"', p.game_mode)}</div>
        <div class="item"><div class="info"><div class="name">Hardware-accelerated GPU scheduling</div><div class="desc">Can lower input lag. Needs a restart.</div></div>${toggle('id="hags"', p.hags)}</div>
      </div></div></div>
      <div class="grid g2" style="margin-top:14px">
        <div class="card"><h3>Visual effects</h3><p class="sub">${v.weak ? "Your PC is on the lighter side, so " : ""}Turning off window animations and fades makes Windows feel snappier. Fonts stay smooth.</p>
          ${v.light ? `<span class="badge red">Lighter visuals on</span>` : `<button class="btn" id="vfx">Use lighter visuals</button>`}
          <div class="item" style="margin-top:8px"><div class="info"><div class="name">Decide automatically</div><div class="desc">Applied once on PCs that need it</div></div>${toggle('data-cfg="visual_auto"', c.visual_auto)}</div></div>
        <div class="card"><h3>Laptop & Windows Update</h3><div class="list">
          <div class="item"><div class="info"><div class="name">Battery mode</div><div class="desc">${pw.battery ? (pw.on_battery ? `On battery (${pw.percent}%) — ` : "") : "No battery found — "}balanced power, effects paused, widgets slowed while unplugged</div></div>${toggle('data-cfg="battery_mode"', c.battery_mode)}</div>
          <div class="item"><div class="info"><div class="name">Updates stay out of the way</div><div class="desc">Restarts only between 02:00 and 08:00, and updates pause while you game</div></div>${toggle('id="wuc"', c.wu_control)}</div>
        </div></div>
      </div>`;
    qa<HTMLButtonElement>("[data-plan]", el).forEach((b) => (b.onclick = () => busy(b, async () => { await call("power_set", { guid: b.dataset.plan }); await SPEED.power(el); })));
    const ult = q<HTMLButtonElement>("#ult", el);
    ult.onclick = () => busy(ult, async () => { await call("power_set", { guid: "ultimate" }); snack("Ultimate Performance on"); await SPEED.power(el); });
    const gm = q<HTMLInputElement>("#gm", el);
    gm.onchange = () => call("game_mode_set", { on: gm.checked });
    const hags = q<HTMLInputElement>("#hags", el);
    hags.onchange = () => call("hags_set", { on: hags.checked }).then(() => snack("Restart your PC to apply"));
    bindCfg(el);
    const vfx = document.getElementById("vfx") as HTMLButtonElement | null;
    if (vfx) vfx.onclick = () => busy(vfx, async () => { await call("visuals_light"); snack("Lighter visuals on"); await SPEED.power(el); });
    const wuc = q<HTMLInputElement>("#wuc", el);
    wuc.onchange = async () => { await setCfg({ wu_control: wuc.checked }); if (wuc.checked) await call("night_restarts"); snack("Saved"); };
  },

  async boot(el) {
    const boots = await call<Any[]>("boot_times");
    if (!boots.length) {
      el.innerHTML = `<div class="card"><div class="empty">Windows hasn't recorded any boot times yet. Restart your PC and check back.</div></div>`;
      return;
    }
    const max = Math.max(...boots.map((b) => b.ms));
    const avg = boots.reduce((s, b) => s + b.ms, 0) / boots.length;
    const last = boots[boots.length - 1].ms;
    el.innerHTML = `<div class="card"><div class="row between"><div><h3>Boot time</h3><p class="sub">Measured by Windows on every start-up. Disable startup apps to bring it down.</p></div>
      <div class="row"><div class="stat"><div class="label">Last</div><div class="value">${(last / 1000).toFixed(1)}<small>s</small></div></div>
      <div class="stat"><div class="label">Average</div><div class="value">${(avg / 1000).toFixed(1)}<small>s</small></div></div></div></div>
      <div class="chart">${boots.map((b, i) => `<div class="${i === boots.length - 1 ? "last" : ""}" style="height:${(b.ms / max) * 100}%" title="${new Date(b.t * 1000).toLocaleString()}"><span>${(b.ms / 1000).toFixed(0)}s</span></div>`).join("")}</div></div>`;
  },
};

/* ================================================================ autopilot */

PAGES.autopilot = {
  title: "Autopilot",
  sub: "Runs from the tray and keeps your PC fast on its own",
  async render(el) {
    const c = await call<Any>("get_config");
    el.innerHTML = `
      <div class="card ${c.autopilot ? "panic" : ""}"><div class="row between">
        <div><h3>Autopilot</h3><p class="sub" style="margin:0">Everything below runs automatically. Each action pops up bottom-right for 5 seconds.</p></div>
        ${toggle('data-cfg="autopilot"', c.autopilot)}</div></div>

      <div class="grid g2" style="margin-top:14px">
        <div class="card"><div class="row between"><h3>Unused app cleaner</h3>${toggle('data-cfg="unused_cleaner"', c.unused_cleaner)}</div>
          <p class="sub">Fully automatic. Uses Windows' own history of what you run (Prefetch, UserAssist, BAM), so it also knows about apps you stopped using long before Nightmoth was installed. Removes them silently, deletes the folder they leave behind, and gives you 5 seconds to cancel each one. Cancelled apps are never touched again. System apps, drivers, games and security software are protected.</p>
          <div class="row"><span class="muted">Remove after</span><select data-cfg="unused_days" data-num>${[30, 60, 90, 180, 365].map((v) => `<option value="${v}" ${v === c.unused_days ? "selected" : ""}>${v} days unused</option>`).join("")}</select></div></div>
        <div class="card"><div class="row between"><h3>Cleanup when it's needed</h3>${toggle('data-cfg="auto_cleanup"', c.auto_cleanup)}</div>
          <p class="sub">No timetable: runs when your drive is getting full or junk has piled up, and does a deep clean while you're away. Last run: ${ago(c.last_cleanup)}.</p>
          <div class="row between"><span>Deep clean (restore points, old drivers, app leftovers…)</span>${toggle('data-cfg="deep_clean"', c.deep_clean)}</div></div>
        <div class="card"><div class="row between"><h3>Repair other tools' damage</h3>${toggle('data-cfg="fix_other_tools"', c.fix_other_tools)}</div>
          <p class="sub">Finds what other optimizers broke (Windows Update, Defender, services, boot "tweaks") and fixes it safely, twice a day. ${c.fixes?.filter((f: Any) => f.action === "fixed").length || 0} repaired so far.</p></div>
        <div class="card"><div class="row between"><h3>Keep apps updated</h3>${toggle('data-cfg="auto_updates"', c.auto_updates)}</div>
          <p class="sub">Installs app updates through winget once a day, while you're away and plugged in.</p></div>
        <div class="card"><div class="row between"><h3>Startup guard</h3>${toggle('data-cfg="startup_watcher"', c.startup_watcher)}</div>
          <p class="sub">When a new app adds itself to startup, Nightmoth turns it off and tells you. Click "Allow" on the pop-up to keep it.</p></div>
        <div class="card"><div class="row between"><h3>Background hog watch</h3>${toggle('data-cfg="hog_watcher"', c.hog_watcher)}</div>
          <p class="sub">When something in the background uses lots of CPU or RAM for 5+ minutes, it goes into Windows 11's Efficiency mode — it keeps working, just without slowing you down.</p>
          <div class="row wrap"><span class="muted">CPU over</span><input type="number" data-cfg="hog_cpu" data-num value="${c.hog_cpu}" style="width:70px">%
            <span class="muted">or RAM over</span><input type="number" data-cfg="hog_ram_mb" data-num value="${c.hog_ram_mb}" style="width:90px">MB</div>
          <div class="row between" style="margin-top:10px"><span>Put them in Efficiency mode automatically</span>${toggle('data-cfg="hog_auto_lower"', c.hog_auto_lower)}</div></div>
      </div>

      <div class="section-title"><h2>Installed apps</h2><div class="row"><input type="text" class="search" id="find" placeholder="Search apps"><label class="row muted"><input type="checkbox" id="onlyunused" style="accent-color:var(--red)"> Only removable</label></div></div>
      <div class="card"><div class="list" id="apps">${loading("Reading app usage")}</div></div>

      <div class="section-title"><h2>Removed by Nightmoth</h2></div>
      <div class="card"><div class="list" id="removed"></div></div>`;
    bindCfg(el);

    const view = await call<Any>("apps_list");
    const removed = q("#removed");
    removed.innerHTML = view.removed.length
      ? view.removed.slice().reverse().map((r: Any) => `
        <div class="item"><div class="info"><div class="name">${esc(r.name)}</div><div class="desc">Removed ${ago(r.date)} · unused ${r.days_unused} days · ${r.size_mb} MB</div></div>
        ${r.reinstalled ? `<span class="badge">Reinstalled</span>` : `<button class="btn small" data-re="${esc(r.name)}">Reinstall</button>`}</div>`).join("")
      : `<div class="empty">Nothing removed yet.</div>`;
    qa<HTMLButtonElement>("[data-re]", removed).forEach((b) => (b.onclick = () => busy(b, () => call("app_reinstall", { name: b.dataset.re }))));

    const renderApps = () => {
      const f = q<HTMLInputElement>("#find").value.toLowerCase();
      const only = q<HTMLInputElement>("#onlyunused").checked;
      const list = view.apps.filter((a: Any) => (!f || a.name.toLowerCase().includes(f)) && (!only || a.candidate));
      q("#apps").innerHTML = list.length
        ? list.map((a: Any) => {
            const used = a.last_used ? `last used ${ago(a.last_used)}` : a.days_unused != null ? `not opened in ${a.days_unused}+ days` : "no usage data";
            const badge = a.protected ? `<span class="badge">Protected</span>` : a.kept ? `<span class="badge white">Kept</span>` : a.candidate ? `<span class="badge red">Will be removed</span>` : "";
            return `<div class="item"><div class="info"><div class="name">${esc(a.name)}</div><div class="desc">${esc(a.publisher || "Unknown publisher")} · ${a.size_mb ? a.size_mb + " MB · " : ""}${used}</div></div>
              ${badge}
              ${a.protected ? "" : `<button class="btn small ghost" data-keep="${esc(a.name)}" data-k="${a.kept ? 0 : 1}">${a.kept ? "Unkeep" : "Keep"}</button>
              <button class="btn small danger" data-rm="${esc(a.key)}" data-n="${esc(a.name)}">Remove</button>`}</div>`;
          }).join("")
        : `<div class="empty">No apps match.</div>`;
      qa<HTMLButtonElement>("[data-keep]").forEach((b) => (b.onclick = async () => {
        await call("app_keep", { name: b.dataset.keep, keep: b.dataset.k === "1" });
        const a = view.apps.find((x: Any) => x.name === b.dataset.keep);
        if (a) { a.kept = b.dataset.k === "1"; a.candidate = a.candidate && !a.kept; }
        renderApps();
      }));
      qa<HTMLButtonElement>("[data-rm]").forEach((b) => (b.onclick = async () => {
        if (!(await confirm(`Uninstall ${b.dataset.n}?`, { title: "Nightmoth", kind: "warning" }))) return;
        busy(b, async () => {
          await call("app_remove", { key: b.dataset.rm });
          view.apps = view.apps.filter((x: Any) => x.key !== b.dataset.rm);
          renderApps();
        });
      }));
    };
    q("#find").oninput = renderApps;
    q("#onlyunused").onchange = renderApps;
    renderApps();
  },
};

/* ================================================================ games */

PAGES.games = {
  title: "Games",
  sub: "Automatic boost every time you play",
  async render(el) {
    const c = await call<Any>("get_config");
    const games = await call<Any[]>("games_list");
    el.innerHTML = `
      <div class="card ${c.game_booster ? "panic" : ""}"><div class="row between"><div><h3>Game booster</h3>
        <p class="sub" style="margin:0">When a game starts: Ultimate Performance power, high priority, notifications paused, chosen apps closed. Everything comes back when you quit.</p></div>
        ${toggle('data-cfg="game_booster"', c.game_booster)}</div></div>

      <div class="grid g2" style="margin-top:14px">
        <div class="card" id="roblox">${loading("Checking Roblox")}</div>
        <div class="card"><h3>While you play</h3><p class="sub">Windows Update downloads are throttled and its services paused; restarts only happen at night.</p>
          <div class="row between"><span>Pause updates while gaming</span>${toggle('data-cfg="wu_control"', c.wu_control)}</div>
          <div class="row" style="margin-top:12px;gap:22px"><div class="stat"><div class="label">Ping</div><div class="value" id="gping">–</div></div><span class="muted" style="font-size:12px">to Cloudflare, updated every few seconds</span></div></div>
      </div>

      <div class="section-title"><h2>Your games</h2><span class="muted">Found automatically</span></div>
      <div class="card"><div class="list">${games.length ? games.map((g) => `
        <div class="item"><div class="info"><div class="name">${esc(g.name)} ${g.running ? `<span class="badge red">Boosting now</span>` : ""}</div>
          <div class="desc">${esc(g.exe)} · played ${g.plays}× · ${Math.round(g.minutes / 60)}h ${g.minutes % 60}m · ${ago(g.last_played)}</div></div>
          ${toggle(`data-g="${esc(g.exe)}"`, g.boost)}</div>`).join("") : `<div class="empty">No games yet — launch one (Roblox, Steam, Epic, Riot…) and it appears here.</div>`}</div></div>

      <div class="section-title"><h2>Close while gaming</h2><button class="btn small" id="add">Add a running app</button></div>
      <div class="card"><p class="sub">These get closed when a game starts and reopened when you stop.</p>
        <div class="row wrap" id="closelist"></div><div id="picker"></div></div>`;
    bindCfg(el);
    qa<HTMLInputElement>("[data-g]", el).forEach((t) => (t.onchange = () => call("game_boost_set", { exe: t.dataset.g, on: t.checked })));

    const drawRoblox = async () => {
      const box = q("#roblox");
      if (!box) return;
      const r = await raw<Any>("roblox_state").catch(() => null);
      if (!r || !r.installed) { box.innerHTML = `<h3>Roblox best settings</h3><p class="sub">Install Roblox and Nightmoth tunes it for your PC automatically.</p>`; return; }
      box.innerHTML = `<div class="row between"><h3>Roblox best settings</h3>${toggle('id="rbon"', c.roblox_tune)}</div>
        <p class="sub">Graphics picked for your hardware${r.tier ? ` (${esc(r.tier)}-end PC)` : ""} and the frame cap raised to your ${r.refresh} Hz screen. Re-applied after every Roblox update. Only settings Roblox allows — nothing injected.</p>
        <div class="row">${r.tuned ? `<span class="badge red">Tuned · ${r.fps} FPS cap</span>` : ""}<button class="btn small" id="rbgo">${r.tuned ? "Tune again" : "Tune now"}</button>${r.tuned ? `<button class="btn small ghost" id="rbundo">Put back original</button>` : ""}</div>`;
      q<HTMLInputElement>("#rbon").onchange = (e) => setCfg({ roblox_tune: (e.target as HTMLInputElement).checked });
      const go2 = q<HTMLButtonElement>("#rbgo");
      go2.onclick = () => busy(go2, async () => { const x = await call<Any>("roblox_apply"); snack(`Roblox: ${x.fps} FPS cap, graphics ${x.quality}/10`); await drawRoblox(); });
      const un = document.getElementById("rbundo") as HTMLButtonElement | null;
      if (un) un.onclick = () => busy(un, async () => { await call("roblox_restore"); snack("Original Roblox settings back"); await drawRoblox(); });
    };
    drawRoblox();
    const gp = async () => {
      const ms = await raw<number | null>("net_ping", { ip: "1.1.1.1" }).catch(() => null);
      const e2 = document.getElementById("gping");
      if (e2) e2.innerHTML = ms == null ? "<small>–</small>" : `${ms}<small> ms</small>`;
    };
    gp();
    timers.push(window.setInterval(gp, 3000));

    let list: string[] = c.boost_close || [];
    const drawList = () => {
      q("#closelist").innerHTML = list.length
        ? list.map((n) => `<span class="badge white" style="padding:5px 10px;text-transform:none">${esc(n)} <a data-x="${esc(n)}" style="cursor:pointer;color:var(--red-glow);margin-left:6px">✕</a></span>`).join("")
        : `<span class="muted">Nothing yet. Good picks: OneDrive, Teams, launchers you don't need in-game.</span>`;
      qa<HTMLElement>("[data-x]").forEach((x) => (x.onclick = async () => {
        list = list.filter((n) => n !== x.dataset.x);
        await setCfg({ boost_close: list });
        drawList();
      }));
    };
    drawList();
    const add = q<HTMLButtonElement>("#add");
    add.onclick = () => busy(add, async () => {
      const procs = await call<Any[]>("processes");
      q("#picker").innerHTML = `<div class="list" style="margin-top:12px;max-height:300px;overflow:auto">${procs
        .filter((p) => !list.includes(p.name))
        .map((p) => `<div class="item"><div class="info"><div class="name">${esc(p.name)}</div><div class="desc">${p.mem_mb} MB</div></div><button class="btn small" data-add="${esc(p.name)}">Add</button></div>`)
        .join("")}</div>`;
      qa<HTMLButtonElement>("[data-add]").forEach((b) => (b.onclick = async () => {
        list.push(b.dataset.add!);
        await setCfg({ boost_close: list });
        b.closest(".item")?.remove();
        drawList();
      }));
    });
  },
};

/* ================================================================ safety */

PAGES.safety = {
  title: "Safety",
  sub: "Everything Nightmoth changed — and how to undo it",
  async render(el) {
    const changes = await call<Any[]>("changes_list");
    const c = await call<Any>("get_config");
    el.innerHTML = `
      <div class="grid g2">
        <div class="card panic"><h3>Undo everything</h3><p class="sub">Reverts every change Nightmoth made, newest first. Removed apps are reinstalled through winget where possible.</p>
          <button class="btn primary" id="panic">Revert all changes</button></div>
        <div class="card"><h3>Restore point</h3><p class="sub">Nightmoth makes one automatically before big changes. Last one: ${ago(c.last_restore_point)}.</p>
          <button class="btn" id="rp">Create restore point now</button></div>
      </div>

      <div class="section-title"><h2>Change history</h2><span class="muted">${changes.length} changes</span></div>
      <div class="card"><div class="list">${changes.length ? changes.map((e) => `
        <div class="item"><span class="badge ${e.module === "Autopilot" ? "red" : ""}">${esc(e.module)}</span>
          <div class="info"><div class="name">${esc(e.title)}</div><div class="desc">${new Date(e.ts * 1000).toLocaleString()}${e.detail ? " · " + esc(e.detail) : ""}</div></div>
          ${e.undone ? `<span class="badge">Undone</span>` : e.undoable ? `<button class="btn small" data-u="${e.id}">Undo</button>` : `<span class="muted" style="font-size:12px">Permanent</span>`}</div>`).join("") : `<div class="empty">No changes yet.</div>`}</div></div>

      <div class="section-title"><h2>Never remove</h2></div>
      <div class="card"><div class="row wrap" id="keep">${(c.keep_list || []).length ? c.keep_list.map((k: string) => `<span class="badge white" style="padding:5px 10px;text-transform:none">${esc(k)} <a data-k="${esc(k)}" style="cursor:pointer;color:var(--red-glow);margin-left:6px">✕</a></span>`).join("") : `<span class="muted">Apps you cancel or mark "Keep" show up here.</span>`}</div></div>`;

    const panic = q<HTMLButtonElement>("#panic");
    panic.onclick = async () => {
      if (!(await confirm("Revert every change Nightmoth has made?", { title: "Nightmoth", kind: "warning" }))) return;
      busy(panic, async () => { const n = await call<number>("undo_all"); snack(`${n} changes undone`); go("safety"); });
    };
    const rp = q<HTMLButtonElement>("#rp");
    rp.onclick = () => busy(rp, async () => { await call("restore_point"); snack("Restore point created"); go("safety"); });
    qa<HTMLButtonElement>("[data-u]").forEach((b) => (b.onclick = () => busy(b, async () => { snack(await call<string>("change_undo", { id: Number(b.dataset.u) })); go("safety"); })));
    qa<HTMLElement>("[data-k]").forEach((x) => (x.onclick = async () => { await call("app_keep", { name: x.dataset.k, keep: false }); go("safety"); }));
  },
};

/* ================================================================ settings */

PAGES.settings = {
  title: "Settings",
  sub: "",
  async render(el) {
    const c = await call<Any>("get_config");
    const ov = await call<Any>("overview");
    const autostart = await call<boolean>("autostart_get");
    el.innerHTML = `
      <div class="grid g2">
        <div class="card"><h3>General</h3><div class="list">
          <div class="item"><div class="info"><div class="name">Start with Windows</div><div class="desc">Opens quietly in the tray when you log in</div></div>${toggle('id="auto"', autostart)}</div>
          <div class="item"><div class="info"><div class="name">Autopilot</div><div class="desc">Automatic optimizations from the tray</div></div>${toggle('data-cfg="autopilot"', c.autopilot)}</div>
        </div></div>
        <div class="card"><h3>Pop-ups</h3><p class="sub">Bottom-right, every time Nightmoth does something. Always on — Autopilot never acts silently.</p><div class="list">
          <div class="item"><div class="info"><div class="name">Show each pop-up for</div></div><input type="range" min="3000" max="10000" step="1000" id="ms" value="${c.toast_ms}"><span class="mono" id="msv">${c.toast_ms / 1000}s</span></div>
        </div><button class="btn" id="test" style="margin-top:10px">Show a test pop-up</button></div>
      </div>
      <div class="section-title"><h2>About</h2></div>
      <div class="card"><div class="row between"><div class="row"><img src="/moth-logo.png" style="width:54px"><div><h3>Nightmoth ${esc(ov.version)}</h3><p class="sub" style="margin:0">Desktop glow-up + autopilot optimizer · by lk5t</p></div></div>
        <button class="btn danger" id="quit">Quit Nightmoth</button></div></div>`;
    bindCfg(el);
    const auto = q<HTMLInputElement>("#auto");
    auto.onchange = () => call("autostart_set", { on: auto.checked }).then(() => snack("Saved")).catch(() => (auto.checked = !auto.checked));
    const ms = q<HTMLInputElement>("#ms");
    ms.oninput = () => (q("#msv").textContent = Number(ms.value) / 1000 + "s");
    ms.onchange = () => setCfg({ toast_ms: Number(ms.value) });
    q("#test").onclick = () => call("toast_test");
    q("#quit").onclick = () => call("quit_app");
  },
};

/* ================================================================ boot */

q("#nav").innerHTML = NAV.map(
  (n) => `${n === "settings" ? '<div class="spacer" style="height:14px"></div>' : ""}<button data-page="${n}">${ICONS[n]}${PAGES[n].title}</button>`
).join("");
qa<HTMLButtonElement>("#nav button").forEach((b) => (b.onclick = () => go(b.dataset.page!)));

function welcome() {
  const content = q("#content");
  q("#title").innerHTML = "Welcome";
  content.innerHTML = `<div class="welcome"><img src="/moth-logo.png" alt=""><h1>NIGHT<span>MOTH</span></h1>
    <p>Pick any wallpaper and Nightmoth themes your whole PC around it. Then it lives in your tray and keeps everything fast on its own — cleaning junk, blocking startup bloat, boosting your games and removing apps you never use.</p>
    <p class="muted" style="font-size:12.5px">Every change is logged and can be undone from Safety.</p>
    <div class="row" style="justify-content:center;margin-top:24px;gap:10px">
      <button class="btn primary big" id="w2">Make my PC perfect</button>
      <button class="btn big" id="w1">Pick a wallpaper first</button>
      <button class="btn big ghost" id="w3">Skip</button></div></div>`;
  const done = () => localStorage.setItem("nm-welcomed", "1");
  q("#w1").onclick = () => { done(); go("look"); };
  q("#w2").onclick = () => { done(); go("smart"); setTimeout(() => q<HTMLButtonElement>("#smartgo")?.click(), 400); };
  q("#w3").onclick = () => { done(); go("home"); };
}

import("@tauri-apps/api/event").then(({ listen }) => listen<string>("open-page", (e) => { if (PAGES[e.payload]) go(e.payload); }));

refreshStatus();
invoke("ui_log", { msg: "main window ready" }).catch(() => {});
if (localStorage.getItem("nm-welcomed")) go("home");
else welcome();
