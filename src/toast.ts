import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./toast.css";

type Toast = { id: string; kind: string; title: string; body: string; action: string | null; ms: number };

const box = document.getElementById("toast")!;
const title = document.getElementById("t-title")!;
const body = document.getElementById("t-body")!;
const action = document.getElementById("t-action") as HTMLButtonElement;
const close = document.getElementById("t-close") as HTMLButtonElement;
const bar = document.getElementById("t-bar")!;

let current: Toast | null = null;
let remaining = 0;
let last = 0;
let hovering = false;
let raf = 0;

function finish() {
  if (!current) return;
  current = null;
  cancelAnimationFrame(raf);
  box.classList.remove("in");
  setTimeout(() => invoke("toast_done"), 230);
}

function tick(t: number) {
  if (!current) return;
  const dt = t - last;
  last = t;
  // hovering pauses the timer, except removal warnings: those must keep counting
  if (!hovering || current.kind === "remove") remaining -= dt;
  bar.style.transform = `scaleX(${Math.max(0, remaining / current.ms)})`;
  if (remaining <= 0) finish();
  else raf = requestAnimationFrame(tick);
}

listen<Toast>("toast", (e) => {
  current = e.payload;
  title.textContent = current.title;
  body.textContent = current.body;
  action.textContent = current.action ?? "";
  remaining = current.ms;
  bar.style.transform = "scaleX(1)";
  box.classList.remove("in");
  void box.offsetWidth;
  box.classList.add("in");
  last = performance.now();
  cancelAnimationFrame(raf);
  raf = requestAnimationFrame(tick);
});

box.addEventListener("mouseenter", () => (hovering = true));
box.addEventListener("mouseleave", () => (hovering = false));

action.addEventListener("click", (e) => {
  e.stopPropagation();
  if (current) invoke("toast_action", { id: current.id });
  finish();
});

close.addEventListener("click", (e) => {
  e.stopPropagation();
  finish();
});

// clicking the card opens Nightmoth
box.addEventListener("click", async () => {
  const { Window } = await import("@tauri-apps/api/window");
  const main = await Window.getByLabel("main");
  if (main) {
    await main.show();
    await main.unminimize();
    await main.setFocus();
  }
  finish();
});

void getCurrentWindow();
invoke("ui_log", { msg: "toast window ready" }).catch(() => {});
