import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./widgets.css";

type State = {
  on: boolean;
  widgets: string[];
  cell: number;
  style: string;
  accent: string;
  dark: boolean;
  scale: number;
  week: { freed_bytes: number; apps_removed: number; games_boosted: number; startup_blocked: number };
  notes: string;
  dock: string[];
  weather_city: string;
  weather_lat: number;
  weather_lon: number;
};
type Media = { title: string; artist: string; playing: boolean; app: string } | null;
type Stats = { cpu: number; ram_used: number; ram_total: number; disk_used: number; disk_total: number; down: number; up: number; uptime: number };

const col = document.getElementById("col")!;
let state: State | null = null;
let order = "";

const pad = (n: number) => String(n).padStart(2, "0");
const gb = (b: number) => (b / 1e9).toFixed(1);
function rate(b: number): string {
  if (b >= 1e6) return (b / 1e6).toFixed(1) + " MB/s";
  if (b >= 1e3) return (b / 1e3).toFixed(0) + " KB/s";
  return b.toFixed(0) + " B/s";
}
function bytes(b: number): string {
  if (b >= 1e9) return (b / 1e9).toFixed(1) + " GB";
  if (b >= 1e6) return (b / 1e6).toFixed(0) + " MB";
  return (b / 1e3).toFixed(0) + " KB";
}

const BUILD: Record<string, () => string> = {
  clock: () => `<div class="w" id="w-clock"><div class="time" id="time"></div><div class="date" id="date"></div></div>`,
  calendar: () => `<div class="w" id="w-cal"><div class="cal-head"><b id="month"></b><span class="label" id="year"></span></div><div class="cal" id="cal"></div></div>`,
  system: () => `<div class="w" id="w-sys"><div class="label">System</div>
    ${["cpu:CPU", "ram:Memory", "disk:Drive"].map((x) => { const [id, n] = x.split(":"); return `<div class="meter"><div class="row"><span>${n}</span><span id="${id}-v">–</span></div><div class="track"><i id="${id}-b"></i></div></div>`; }).join("")}
    <div class="net"><span>↓ <b id="down">–</b></span><span>↑ <b id="up">–</b></span></div></div>`,
  media: () => `<div class="w" id="w-media"><div class="label" id="m-app">Now playing</div><div class="m-title" id="m-title">Nothing playing</div><div class="m-artist" id="m-artist"></div>
    <div class="m-ctl"><button data-m="prev" title="Previous">⏮</button><button data-m="toggle" id="m-play" title="Play / pause">▶</button><button data-m="next" title="Next">⏭</button></div></div>`,
  weather: () => `<div class="w" id="w-wx"><div class="cal-head"><b id="wx-city">Weather</b><span class="label" id="wx-desc"></span></div>
    <div class="wx-row"><span class="wx-icon" id="wx-icon"></span><span class="wx-temp" id="wx-temp">–</span><span class="wx-range" id="wx-range"></span></div></div>`,
  notes: () => `<div class="w" id="w-notes"><div class="label">Notes</div><div class="notes" id="notes"></div></div>`,
  dock: () => `<div class="w" id="w-dock"><div class="dock" id="dock"></div></div>`,
  week: () => `<div class="w" id="w-week"><div class="label">Nightmoth this week</div><div class="stats">
    <div><b id="wk-freed">–</b><span>freed</span></div><div><b id="wk-apps">–</b><span>apps removed</span></div>
    <div><b id="wk-games">–</b><span>games boosted</span></div><div><b id="wk-start">–</b><span>startup blocked</span></div></div></div>`,
};

function clock() {
  const t = document.getElementById("time");
  if (!t) return;
  const d = new Date();
  t.innerHTML = `${pad(d.getHours())}:${pad(d.getMinutes())}<span class="sec">${pad(d.getSeconds())}</span>`;
  document.getElementById("date")!.innerHTML = `<b>${d.toLocaleDateString(undefined, { weekday: "long" })}</b> · ${d.toLocaleDateString(undefined, { day: "numeric", month: "long" })}`;
}

let calDay = -1;
function calendar() {
  const c = document.getElementById("cal");
  if (!c) return;
  const now = new Date();
  if (calDay === now.getDate() && c.childElementCount) return;
  calDay = now.getDate();
  const y = now.getFullYear(), m = now.getMonth();
  document.getElementById("month")!.textContent = now.toLocaleDateString(undefined, { month: "long" });
  document.getElementById("year")!.textContent = String(y);
  const first = (new Date(y, m, 1).getDay() + 6) % 7; // Monday first
  const days = new Date(y, m + 1, 0).getDate();
  const prev = new Date(y, m, 0).getDate();
  const dows = ["M", "T", "W", "T", "F", "S", "S"].map((d) => `<span class="dow">${d}</span>`).join("");
  let cells = "";
  for (let i = 0; i < 42; i++) {
    const n = i - first + 1;
    if (n < 1) cells += `<span class="out">${prev + n}</span>`;
    else if (n > days) { if (i % 7 === 0) break; cells += `<span class="out">${n - days}</span>`; }
    else cells += `<span class="${n === now.getDate() ? "today" : i % 7 >= 5 ? "wkend" : ""}">${n}</span>`;
  }
  c.innerHTML = dows + cells;
}

function week() {
  if (!state || !document.getElementById("w-week")) return;
  const w = state.week;
  document.getElementById("wk-freed")!.textContent = bytes(w.freed_bytes || 0);
  document.getElementById("wk-apps")!.textContent = String(w.apps_removed);
  document.getElementById("wk-games")!.textContent = String(w.games_boosted);
  document.getElementById("wk-start")!.textContent = String(w.startup_blocked);
}

const WX: Record<number, [string, string]> = {
  0: ["☀", "Clear"], 1: ["🌤", "Mostly clear"], 2: ["⛅", "Partly cloudy"], 3: ["☁", "Cloudy"], 45: ["🌫", "Fog"], 48: ["🌫", "Fog"],
  51: ["🌦", "Drizzle"], 53: ["🌦", "Drizzle"], 55: ["🌦", "Drizzle"], 61: ["🌧", "Rain"], 63: ["🌧", "Rain"], 65: ["🌧", "Heavy rain"],
  71: ["🌨", "Snow"], 73: ["🌨", "Snow"], 75: ["❄", "Heavy snow"], 80: ["🌦", "Showers"], 81: ["🌧", "Showers"], 82: ["⛈", "Storms"],
  95: ["⛈", "Thunder"], 96: ["⛈", "Thunder"], 99: ["⛈", "Thunder"],
};
let wxAt = 0;
let wxKey = "";
async function weather(force = false) {
  if (!state || !document.getElementById("w-wx")) return;
  const key = `${state.weather_lat},${state.weather_lon}`;
  if (!state.weather_lat && !state.weather_lon) {
    document.getElementById("wx-city")!.textContent = "Weather";
    document.getElementById("wx-desc")!.textContent = "set a city in Look";
    return;
  }
  if (!force && key === wxKey && Date.now() - wxAt < 20 * 60 * 1000) return;
  wxKey = key;
  wxAt = Date.now();
  try {
    const r = await fetch(`https://api.open-meteo.com/v1/forecast?latitude=${state.weather_lat}&longitude=${state.weather_lon}&current=temperature_2m,weather_code&daily=temperature_2m_max,temperature_2m_min&forecast_days=1&timezone=auto`).then((x) => x.json());
    const [icon, text] = WX[r.current.weather_code] ?? ["·", ""];
    document.getElementById("wx-city")!.textContent = state.weather_city.split(",")[0];
    document.getElementById("wx-desc")!.textContent = text;
    document.getElementById("wx-icon")!.textContent = icon;
    document.getElementById("wx-temp")!.textContent = `${Math.round(r.current.temperature_2m)}°`;
    document.getElementById("wx-range")!.textContent = `${Math.round(r.daily.temperature_2m_max[0])}° / ${Math.round(r.daily.temperature_2m_min[0])}°`;
  } catch {
    wxAt = Date.now() - 18 * 60 * 1000; // try again in 2 minutes
  }
}

function notesAndDock() {
  if (!state) return;
  const n = document.getElementById("notes");
  if (n) n.textContent = state.notes || "Write notes in Nightmoth → Look";
  const d = document.getElementById("dock");
  if (d && d.dataset.key !== state.dock.join("|")) {
    d.dataset.key = state.dock.join("|");
    d.innerHTML = state.dock.length
      ? state.dock.map((p) => { const name = (p.split("\\").pop() || p).replace(/\.(exe|lnk|url)$/i, ""); return `<button class="app" data-path="${p.replace(/"/g, "&quot;")}" title="${name}"><i>${name.charAt(0).toUpperCase()}</i><span>${name}</span></button>`; }).join("")
      : `<span class="label">Add apps in Nightmoth → Look</span>`;
    d.querySelectorAll<HTMLButtonElement>("button.app").forEach((b) => (b.onclick = () => invoke("dock_launch", { path: b.dataset.path }).catch(() => {})));
  }
}

function reportSize() {
  const r = col.getBoundingClientRect();
  invoke("widgets_size", { w: Math.ceil(r.width) + 2, h: Math.ceil(r.height) + 2 }).catch(() => {});
}
new ResizeObserver(reportSize).observe(col);

const l1 = listen<State>("widgets-state", (e) => {
  state = e.payload;
  const root = document.documentElement;
  root.style.setProperty("--acc", state.accent);
  root.style.setProperty("--scale", String(state.scale));
  document.body.className = `style-${state.style} ${state.dark ? "dark" : "light"}`;
  col.classList.toggle("right", state.cell % 3 === 2);
  const key = state.widgets.join(",");
  if (key !== order) {
    order = key;
    calDay = -1;
    col.innerHTML = state.widgets.filter((w) => BUILD[w]).map((w) => BUILD[w]()).join("");
    clock();
    calendar();
    wxKey = "";
    document.querySelectorAll<HTMLButtonElement>("[data-m]").forEach((b) => (b.onclick = () => invoke("media_control", { action: b.dataset.m }).catch(() => {})));
  }
  week();
  notesAndDock();
  weather();
  reportSize();
});

const l2 = listen<Stats>("widgets-stats", (e) => {
  const s = e.payload;
  const set = (id: string, v: string, pct: number) => {
    const el = document.getElementById(id + "-v");
    if (!el) return;
    el.textContent = v;
    (document.getElementById(id + "-b") as HTMLElement).style.width = Math.min(100, Math.max(0, pct)) + "%";
  };
  set("cpu", s.cpu.toFixed(0) + "%", s.cpu);
  set("ram", `${gb(s.ram_used)} / ${gb(s.ram_total)} GB`, (s.ram_used / s.ram_total) * 100);
  if (s.disk_total) set("disk", `${((s.disk_used / s.disk_total) * 100).toFixed(0)}% used`, (s.disk_used / s.disk_total) * 100);
  const d = document.getElementById("down");
  if (d) { d.textContent = rate(s.down); document.getElementById("up")!.textContent = rate(s.up); }
});

setInterval(() => { clock(); calendar(); }, 1000);
const l3 = listen<Media>("widgets-media", (e) => {
  const m = e.payload;
  const t = document.getElementById("m-title");
  if (!t) return;
  t.textContent = m ? m.title : "Nothing playing";
  document.getElementById("m-artist")!.textContent = m ? m.artist : "";
  document.getElementById("m-app")!.textContent = m && m.app ? `Now playing · ${m.app}` : "Now playing";
  document.getElementById("m-play")!.textContent = m && m.playing ? "⏸" : "▶";
});
setInterval(() => weather(), 60 * 1000);

// only say "ready" once the listeners exist, so the first state isn't lost
Promise.all([l1, l2, l3]).then(() => invoke("widgets_ready")).catch(() => {});
