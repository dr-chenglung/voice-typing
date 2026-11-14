// 關於頁：版本號與使用提醒。
import { escapeHtml as esc } from "../lib/html.js";

export async function load(section) {
  let version = "未知";
  try {
    version = await window.__TAURI__.app.getVersion();
  } catch {
    /* 取不到版本號就顯示「未知」 */
  }
  section.innerHTML = `
    <h1 class="page-title">關於</h1>
    <div class="card">
      <h2 class="card-title">VoiceTyping</h2>
      <p>版本 ${esc(version)}</p>
      <p>按熱鍵說話，自動轉成文字並輸入到目前的視窗。</p>
      <p class="hint">新舊版本不能同時執行，兩者會使用同一個熱鍵。</p>
    </div>`;
}

export function isDirty() {
  return false;
}
