// 主頁：目前狀態（含翻譯模式開關）、目前使用的設定（附「更換」連結）、最近一筆輸出。
// 統計與 API 用量卡片屬於第四階段，這裡先不做。
import * as api from "../api.js";
import { escapeHtml as esc } from "../lib/html.js";
import { hotkeyLabel, STYLES, styleIdFromFormatting } from "../lib/settings.js";
import { latestEntry } from "../lib/history.js";

const STATE_LABELS = { idle: "閒置", recording: "錄音中", processing: "處理中" };

let section = null;
let ctx = null;
let listening = false;

function infoRow(label, value, goTo) {
  const link = goTo ? `<button type="button" class="link" data-go="${goTo}">更換</button>` : "";
  return `<div class="info-row"><span class="info-label">${esc(label)}</span><span class="info-value">${esc(value)}</span>${link}</div>`;
}

async function draw() {
  const [cfg, status, history] = await Promise.all([api.getConfig(), api.getStatus(), api.getHistory()]);
  const latest = latestEntry(history);
  const style = STYLES.find((s) => s.id === styleIdFromFormatting(cfg.enable_formatting));
  const mode = cfg.translate_mode_active ? `翻譯模式 → ${cfg.target_language}` : "一般模式";

  const latestHtml = latest
    ? `<p class="latest-text">${esc(latest.text)}</p>
       <button type="button" class="secondary small" id="copy_latest">複製</button>
       <span class="status-text" id="copy_status" role="status"></span>`
    : `<p class="hint">還沒有任何紀錄</p>`;

  section.innerHTML = `
    <h1 class="page-title">主頁</h1>
    <p class="page-desc">按下熱鍵開始錄音，再按一次結束，文字會自動輸入到目前的視窗。</p>
    <div class="card">
      <h2 class="card-title">目前狀態</h2>
      ${infoRow("狀態", `${STATE_LABELS[status.state] ?? "閒置"} · ${mode}`)}
      ${infoRow("熱鍵", hotkeyLabel(cfg.hotkey))}
      <label class="checkbox"><input type="checkbox" id="home_translate"${cfg.translate_mode_active ? " checked" : ""} />翻譯模式（下次錄音翻譯成 ${esc(cfg.target_language)}）</label>
      <span class="status-text" id="home_status" role="status"></span>
    </div>
    <div class="card">
      <h2 class="card-title">目前使用</h2>
      ${infoRow("語音辨識", cfg.stt_model, "stt")}
      ${infoRow("AI 整理", cfg.enable_correction ? cfg.llm_model : "AI 整理已關閉", "llm")}
      ${infoRow("整理風格", style.name, "style")}
    </div>
    <div class="card">
      <h2 class="card-title">最近一筆</h2>
      ${latestHtml}
    </div>`;

  for (const btn of section.querySelectorAll("button[data-go]")) {
    btn.addEventListener("click", () => ctx.navigate(btn.dataset.go));
  }

  const toggle = section.querySelector("#home_translate");
  toggle.addEventListener("change", async () => {
    const wanted = toggle.checked;
    try {
      await api.saveFields({ translate_mode_active: wanted });
    } catch (err) {
      toggle.checked = !wanted;
      const el = section.querySelector("#home_status");
      el.textContent = `切換失敗：${err}`;
      el.classList.add("error");
      return;
    }
    ctx.refreshStatus();
    await draw();
  });

  const copyBtn = section.querySelector("#copy_latest");
  if (copyBtn) {
    copyBtn.addEventListener("click", async () => {
      const el = section.querySelector("#copy_status");
      try {
        await navigator.clipboard.writeText(latest.text);
        el.textContent = "已複製";
        el.classList.remove("error");
      } catch (err) {
        el.textContent = `複製失敗：${err}`;
        el.classList.add("error");
      }
    });
  }
}

export async function load(sec, context) {
  section = sec;
  ctx = context;
  await draw();
  if (!listening) {
    listening = true;
    // 主頁正在顯示時，狀態或歷史有變就重畫；沒在顯示時切回來會自己重新載入。
    const refresh = () => {
      if (section && !section.hidden) draw().catch(() => {});
    };
    await api.listen("app-status", refresh);
    await api.listen("history-updated", refresh);
    await api.listen("history-cleared", refresh);
  }
}

export function isDirty() {
  return false;
}
