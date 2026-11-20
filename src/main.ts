import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, confirm } from "@tauri-apps/plugin-dialog";
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
const LOCAL = new Set(["ui_log", "autostart_get", "autostart_set", "quit_app", "toast_done", "toast_action"]);

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
const NAV = ["home", "look", "speed", "autopilot", "games", "safety", "settings"];

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
          <button class="btn primary big" id="optimize" style="width:100%;justify-content:center">Optimize now</button>
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
    const accent = st.accent || a?.accent || "#E10600";
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
        <div class="preset" data-preset="void"><b>Void</b><span>Pure black-red wallpaper, hidden taskbar, empty desktop</span></div>
        <div class="preset" data-preset="neon"><b>Neon Night</b><span>Synthwave wallpaper, hidden taskbar & icons</span></div>
        <div class="preset" data-preset="clean"><b>Clean</b><span>Keep your wallpaper, strip the clutter</span></div>
        <div class="preset" data-preset="default"><b>Windows default</b><span>Put the taskbar and icons back</span></div>
      </div>

      <div class="section-title"><h2>Desktop & taskbar</h2></div>
      <div class="card"><div class="list">
        <div class="item"><div class="info"><div class="name">Hide taskbar automatically</div><div class="desc">Slides up when your mouse touches the bottom</div></div>${toggle('data-desk="autohide"', d.autohide)}</div>
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
      ["startup", "Startup"], ["cleanup", "Cleanup"], ["debloat", "Debloat"], ["services", "Services"],
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
    const on = items.filter((i) => i.enabled).length;
    el.innerHTML = `<div class="card"><div class="row between"><div><h3>Apps that start with Windows</h3>
      <p class="sub">${on} of ${items.length} enabled. Turning things off here is the #1 way to boot faster. Autopilot blocks new ones automatically.</p></div></div>
      <div class="list">${items.map((i) => `
        <div class="item"><div class="info"><div class="name">${esc(i.name)}</div><div class="desc" title="${esc(i.command)}">${esc(i.location)} · ${esc(i.command)}</div></div>
          <span class="badge ${i.impact === "High" ? "red" : ""}">${esc(i.impact)}</span>${toggle(`data-id="${esc(i.id)}"`, i.enabled)}</div>`).join("") || `<div class="empty">Nothing starts with Windows. Nice.</div>`}
      </div></div>`;
    qa<HTMLInputElement>("[data-id]", el).forEach((t) => {
      t.onchange = () => call("startup_set", { id: t.dataset.id, enabled: t.checked }).then(() => snack("Saved")).catch(() => (t.checked = !t.checked));
    });
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
    el.innerHTML = `<div class="grid g2">
      <div class="card"><h3>Power plan</h3><p class="sub">Ultimate Performance keeps the CPU at full speed. Game boosts switch to it automatically.</p>
        <div class="list">${p.plans.map((pl: Any) => `
          <div class="item"><div class="info"><div class="name">${esc(pl.name || pl.guid)}</div><div class="desc mono">${esc(pl.guid)}</div></div>
          ${pl.active ? `<span class="badge red">Active</span>` : `<button class="btn small" data-plan="${pl.guid}">Use</button>`}</div>`).join("")}</div>
        <button class="btn primary" id="ult" style="margin-top:12px">Use Ultimate Performance</button></div>
      <div class="card"><h3>Gaming</h3><p class="sub">Windows' own performance switches.</p><div class="list">
        <div class="item"><div class="info"><div class="name">Windows Game Mode</div><div class="desc">Prioritises games, pauses Windows Update installs while playing</div></div>${toggle('id="gm"', p.game_mode)}</div>
        <div class="item"><div class="info"><div class="name">Hardware-accelerated GPU scheduling</div><div class="desc">Can lower input lag. Needs a restart.</div></div>${toggle('id="hags"', p.hags)}</div>
      </div></div></div>`;
    qa<HTMLButtonElement>("[data-plan]", el).forEach((b) => (b.onclick = () => busy(b, async () => { await call("power_set", { guid: b.dataset.plan }); await SPEED.power(el); })));
    const ult = q<HTMLButtonElement>("#ult", el);
    ult.onclick = () => busy(ult, async () => { await call("power_set", { guid: "ultimate" }); snack("Ultimate Performance on"); await SPEED.power(el); });
    const gm = q<HTMLInputElement>("#gm", el);
    gm.onchange = () => call("game_mode_set", { on: gm.checked });
    const hags = q<HTMLInputElement>("#hags", el);
    hags.onchange = () => call("hags_set", { on: hags.checked }).then(() => snack("Restart your PC to apply"));
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
          <p class="sub">Fully automatic. Removes apps you haven't opened in a long time. You get 5 seconds to cancel each one, and cancelled apps are never touched again. System apps, drivers, games and security software are protected.</p>
          <div class="row"><span class="muted">Remove after</span><select data-cfg="unused_days" data-num>${[30, 60, 90, 180, 365].map((v) => `<option value="${v}" ${v === c.unused_days ? "selected" : ""}>${v} days unused</option>`).join("")}</select></div></div>
        <div class="card"><div class="row between"><h3>Scheduled cleanup</h3>${toggle('data-cfg="auto_cleanup"', c.auto_cleanup)}</div>
          <p class="sub">Clears temp files, update leftovers and caches. Last run: ${ago(c.last_cleanup)}.</p>
          <div class="row"><span class="muted">Every</span><select data-cfg="cleanup_every_days" data-num>${[1, 3, 7, 14, 30].map((v) => `<option value="${v}" ${v === c.cleanup_every_days ? "selected" : ""}>${v === 1 ? "day" : v + " days"}</option>`).join("")}</select></div></div>
        <div class="card"><div class="row between"><h3>Startup guard</h3>${toggle('data-cfg="startup_watcher"', c.startup_watcher)}</div>
          <p class="sub">When a new app adds itself to startup, Nightmoth turns it off and tells you. Click "Allow" on the pop-up to keep it.</p></div>
        <div class="card"><div class="row between"><h3>Background hog watch</h3>${toggle('data-cfg="hog_watcher"', c.hog_watcher)}</div>
          <p class="sub">Warns when something in the background uses lots of CPU or RAM for 5+ minutes.</p>
          <div class="row wrap"><span class="muted">CPU over</span><input type="number" data-cfg="hog_cpu" data-num value="${c.hog_cpu}" style="width:70px">%
            <span class="muted">or RAM over</span><input type="number" data-cfg="hog_ram_mb" data-num value="${c.hog_ram_mb}" style="width:90px">MB</div>
          <div class="row between" style="margin-top:10px"><span>Slow them down automatically</span>${toggle('data-cfg="hog_auto_lower"', c.hog_auto_lower)}</div></div>
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

const TOAST_TYPES: [string, string][] = [
  ["theme", "Theme changes"], ["optimize", "Optimizations"], ["cleanup", "Cleanups"], ["startup", "Startup blocked"],
  ["game", "Game boosts"], ["hog", "Background hogs"], ["report", "Weekly report"],
];

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
        <div class="card"><h3>Pop-ups</h3><div class="list">
          <div class="item"><div class="info"><div class="name">Show pop-ups</div><div class="desc">Bottom-right, like Razer Cortex</div></div>${toggle('data-cfg="toast_enabled"', c.toast_enabled)}</div>
          <div class="item"><div class="info"><div class="name">Quiet mode</div><div class="desc">Hide everything except removal warnings</div></div>${toggle('data-cfg="toast_quiet"', c.toast_quiet)}</div>
          <div class="item"><div class="info"><div class="name">Show for</div></div><input type="range" min="3000" max="10000" step="1000" id="ms" value="${c.toast_ms}"><span class="mono" id="msv">${c.toast_ms / 1000}s</span></div>
        </div><button class="btn" id="test" style="margin-top:10px">Show a test pop-up</button></div>
      </div>
      <div class="section-title"><h2>Pop-up types</h2></div>
      <div class="card"><div class="list">${TOAST_TYPES.map(([id, n]) => `<div class="item"><div class="info"><div class="name">${n}</div></div>${toggle(`data-tt="${id}"`, !(c.toast_off || []).includes(id))}</div>`).join("")}
        <div class="item"><div class="info"><div class="name">App removal warnings</div><div class="desc">Always shown so you can cancel</div></div><span class="badge red">Always on</span></div></div></div>
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
    qa<HTMLInputElement>("[data-tt]").forEach((t) => (t.onchange = async () => {
      const off = new Set<string>(c.toast_off || []);
      t.checked ? off.delete(t.dataset.tt!) : off.add(t.dataset.tt!);
      c.toast_off = [...off];
      await setCfg({ toast_off: c.toast_off });
    }));
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
      <button class="btn primary big" id="w1">Pick a wallpaper</button>
      <button class="btn big" id="w2">Optimize my PC</button>
      <button class="btn big ghost" id="w3">Skip</button></div></div>`;
  const done = () => localStorage.setItem("nm-welcomed", "1");
  q("#w1").onclick = () => { done(); go("look"); };
  q("#w2").onclick = () => { done(); go("home"); setTimeout(() => q<HTMLButtonElement>("#optimize")?.click(), 600); };
  q("#w3").onclick = () => { done(); go("home"); };
}

refreshStatus();
invoke("ui_log", { msg: "main window ready" }).catch(() => {});
if (localStorage.getItem("nm-welcomed")) go("home");
else welcome();
