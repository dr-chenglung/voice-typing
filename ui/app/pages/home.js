// 主頁：設定提醒（缺東西時才出現）、目前使用的設定（附「更換」連結，翻譯模式可直接開關）、最近一筆輸出。
// 目前的錄音狀態由左下角的狀態列負責，這頁不再重複顯示。統計與 API 用量屬於之後的階段，先不做。
import * as api from "../api.js";
import { escapeHtml as esc } from "../lib/html.js";
import { hotkeyLabel, STYLE_CHOICES, styleChoiceFromConfig, endpointHost, setupWarnings } from "../lib/settings.js";
import { latestEntry } from "../lib/history.js";

let section = null;
let ctx = null;
let listening = false;

function changeButton(goTo) {
  return `<button type="button" class="link" data-go="${esc(goTo)}">更換</button>`;
}

function infoRow(label, value, goTo) {
  return `<div class="info-row"><span class="info-label">${esc(label)}</span><span class="info-value">${esc(value)}</span>${changeButton(goTo)}</div>`;
}

/** 「模型 · 主機」，主機取自服務網址，一眼看得出是雲端還是自己的本機服務。 */
function modelWithHost(model, url) {
  return [model, endpointHost(url)].filter(Boolean).join(" · ");
}

async function draw() {
  const [cfg, history] = await Promise.all([api.getConfig(), api.getHistory()]);
  const latest = latestEntry(history);
  const style = STYLE_CHOICES.find((c) => c.id === styleChoiceFromConfig(cfg));
  const warnings = setupWarnings(cfg);
  // 整理用的語言模型連線：不使用 AI 整理、而且翻譯也關著，才算「未使用」（翻譯會用到同一組連線）。
  const llmUnused = !cfg.enable_correction && !cfg.translate_mode_active;

  const warningHtml = warnings.length
    ? `<div class="card warning">
        <h2 class="card-title">設定提醒</h2>
        ${warnings.map((w) => `<div class="warning-row"><span class="info-value">${esc(w.message)}</span><button type="button" class="link" data-go="${esc(w.goTo)}">前往設定</button></div>`).join("")}
      </div>`
    : "";

  const latestHtml = latest
    ? `<p class="latest-text">${esc(latest.text)}</p>
       <button type="button" class="secondary small" id="copy_latest">複製</button>
       <span class="status-text" id="copy_status" role="status"></span>`
    : `<p class="hint">還沒有任何紀錄</p>`;

  section.innerHTML = `
    <h1 class="page-title">主頁</h1>
    <p class="page-desc">按下 ${esc(hotkeyLabel(cfg.hotkey))} 開始錄音，再按一次結束，文字會自動輸入到目前的視窗。</p>
    ${warningHtml}
    <div class="card">
      <h2 class="card-title">目前使用</h2>
      ${infoRow("語音辨識 API", modelWithHost(cfg.stt_model, cfg.stt_api_url), "stt")}
      ${infoRow("AI 整理 API", llmUnused ? "未使用" : modelWithHost(cfg.llm_model, cfg.llm_api_url), "llm")}
      ${infoRow("AI 整理風格", style.name, "style")}
      <div class="info-row">
        <span class="info-label">翻譯模式</span>
        <span class="info-value"><label class="checkbox"><input type="checkbox" id="home_translate"${cfg.translate_mode_active ? " checked" : ""} />目標語言：${esc(cfg.target_language)}</label></span>
        ${changeButton("translate")}
      </div>
      <span class="status-text" id="home_status" role="status"></span>
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
    // 主頁正在顯示時，歷史有變就重畫；沒在顯示時切回來會自己重新載入。
    const refresh = () => {
      if (section && !section.hidden) draw().catch(() => {});
    };
    await api.listenSafely("history-updated", refresh);
    await api.listenSafely("history-cleared", refresh);
  }
}

export function isDirty() {
  return false;
}
