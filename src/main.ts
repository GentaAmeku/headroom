import { invoke } from "@tauri-apps/api/core";

type UsageWindow = { label: string; consumption: number; resets_at: string };
type ToolSnapshot = {
  tool: string;
  display_name: string;
  status: "ok" | "failed" | "unconnected";
  windows: UsageWindow[];
  error?: string | null;
};

const DOT_CLASS: Record<string, string> = {
  claude: "dot--claude",
  cursor: "dot--cursor",
  codex: "dot--codex",
};
const DOT_LABEL: Record<string, string> = { claude: "C", cursor: "Cu", codex: "Co" };

// design.md: 0-69 ok / 70-89 warn / 90-100 danger
function statusClass(pct: number): string {
  return pct >= 90 ? "s-danger" : pct >= 70 ? "s-warn" : "s-ok";
}

function fmtReset(iso: string): string {
  const ms = new Date(iso).getTime() - Date.now();
  let s = Math.floor(ms / 1000);
  if (Number.isNaN(s) || s <= 0) return "まもなくリセット";
  const d = Math.floor(s / 86400);
  s %= 86400;
  const h = Math.floor(s / 3600);
  s %= 3600;
  const m = Math.floor(s / 60);
  const head = d > 0 ? `${d}日${h}時間` : h > 0 ? `${h}時間${m}分` : `${m}分`;
  return `${head}後にリセット`;
}

function esc(v: string): string {
  return v.replace(/[&<>"]/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] as string,
  );
}

function cardHtml(t: ToolSnapshot): string {
  const head = `<div class="card__head">
    <span class="dot ${DOT_CLASS[t.tool] ?? ""}">${DOT_LABEL[t.tool] ?? "?"}</span>
    <span class="card__name">${esc(t.display_name)}</span>
    <span class="card__state">${t.status === "ok" ? "最新" : ""}</span>
  </div>`;

  if (t.status === "unconnected") {
    return head + `<div class="banner">未接続 — ${esc(t.display_name)} にログインすると表示されます</div>`;
  }
  if (t.status === "failed") {
    return head + `<div class="banner">⚠ 更新に失敗（${esc(t.error ?? "エラー")}）<button class="retry" type="button">再試行</button></div>`;
  }
  const wins = t.windows
    .map((w) => {
      const st = statusClass(w.consumption);
      return `<div class="win ${st}">
        <div class="win__top"><span class="win__label">${esc(w.label)}</span><span class="win__pct">${Math.round(w.consumption)}%</span></div>
        <div class="bar"><i style="--p:${w.consumption}%"></i></div>
        <div class="win__reset">${fmtReset(w.resets_at)}</div>
      </div>`;
    })
    .join("");
  return head + wins;
}

function render(tools: ToolSnapshot[]) {
  const root = document.getElementById("tools");
  if (!root) return;
  root.innerHTML = "";
  for (const t of tools) {
    const card = document.createElement("div");
    card.className =
      "card" +
      (t.status === "failed" ? " is-failed" : "") +
      (t.status === "unconnected" ? " is-unconnected" : "");
    card.innerHTML = cardHtml(t);
    root.appendChild(card);
  }
}

async function refresh() {
  const meta = document.getElementById("meta");
  const foot = document.getElementById("foot-status");
  try {
    const tools = await invoke<ToolSnapshot[]>("get_snapshots");
    render(tools);
    const now = new Date().toLocaleTimeString("ja-JP", { hour: "2-digit", minute: "2-digit" });
    if (meta) meta.textContent = `⟳ ${now}`;
    if (foot) foot.textContent = `更新 ${now}`;
  } catch (e) {
    if (foot) foot.textContent = `エラー: ${String(e)}`;
  }
}

window.addEventListener("DOMContentLoaded", () => {
  document.getElementById("quit")?.addEventListener("click", () => invoke("quit_app"));
  document.getElementById("tools")?.addEventListener("click", (e) => {
    if ((e.target as HTMLElement)?.classList.contains("retry")) refresh();
  });
  refresh();
});

// ポップオーバーが再表示されてフォーカスを得たら更新（stale-while-revalidate）
window.addEventListener("focus", refresh);
