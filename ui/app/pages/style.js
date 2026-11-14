// 整理風格頁：點卡片切換基本型／積極型，立即存檔（對應舊的 enable_formatting 開關）。
// 自訂風格留到第三階段。
import * as api from "../api.js";
import { STYLES, styleIdFromFormatting, formattingFromStyleId } from "../lib/settings.js";
import { escapeHtml as esc } from "../lib/html.js";

let section = null;
let ctx = null;

function setStatus(text, isError = false) {
  const el = section.querySelector(".status-text");
  el.textContent = text;
  el.classList.toggle("error", isError);
}

async function draw(note = "") {
  const cfg = await api.getConfig();
  const active = styleIdFromFormatting(cfg.enable_formatting);
  const cards = STYLES.map(
    (style) => `<button type="button" class="style-card${style.id === active ? " active" : ""}" data-id="${esc(style.id)}">
      <span class="style-name">${esc(style.name)}${style.id === active ? '<span class="badge">使用中</span>' : ""}</span>
      <span class="style-desc">${esc(style.description)}</span>
    </button>`,
  ).join("");
  section.innerHTML = `
    <h1 class="page-title">整理風格</h1>
    <p class="page-desc">選擇 AI 整理文字時的風格。點一下卡片即可切換，立即生效。</p>
    <div class="style-list">${cards}</div>
    <p class="hint">需在「AI 整理」頁啟用 AI 整理才會生效；翻譯模式另有自己的排版方式。</p>
    <span class="status-text" role="status"></span>`;
  for (const btn of section.querySelectorAll(".style-card")) {
    btn.addEventListener("click", () => choose(btn.dataset.id));
  }
  if (note) setStatus(note);
}

async function choose(id) {
  try {
    await api.saveFields({ enable_formatting: formattingFromStyleId(id) });
  } catch (err) {
    setStatus(`切換失敗：${err}`, true);
    return;
  }
  await draw("已切換");
  ctx.refreshStatus();
}

export async function load(sec, context) {
  section = sec;
  ctx = context;
  await draw();
}

export function isDirty() {
  return false;
}
