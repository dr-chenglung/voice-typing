// 設定類頁面共用的流程：讀設定 → 畫表單 → 存檔 → 顯示結果，以及「有沒有未存修改」的判斷。
// 每一頁只需要描述自己的欄位（render）與怎麼從畫面讀出欄位值（read）。
// 未存修改的判斷方式：把目前畫面上的值，和剛載入（或剛存檔）時讀到的值比較。

import * as api from "./api.js";
import { escapeHtml as esc } from "./lib/html.js";

// ---- 產生欄位 HTML 的小工具（所有放進 HTML 的文字都先跳脫） ----

export function textInput(id, label, value, placeholder = "") {
  return `<label>${esc(label)}<input type="text" id="${id}" value="${esc(value)}" placeholder="${esc(placeholder)}" spellcheck="false" /></label>`;
}

/** API 金鑰欄位：預設以圓點遮住，右側眼睛按鈕切換顯示（行為與舊設定視窗相同）。 */
export function keyInput(id, label, value, placeholder = "") {
  return `<label>${esc(label)}
    <span class="key-row">
      <input type="password" id="${id}" value="${esc(value)}" placeholder="${esc(placeholder)}" autocomplete="off" spellcheck="false" />
      <button type="button" class="reveal" data-target="${id}" title="顯示金鑰" aria-label="顯示金鑰" aria-pressed="false"></button>
    </span>
  </label>`;
}

export function checkbox(id, label, checked) {
  return `<label class="checkbox"><input type="checkbox" id="${id}"${checked ? " checked" : ""} />${esc(label)}</label>`;
}

export function hint(text) {
  return `<p class="hint">${esc(text)}</p>`;
}

export function card(title, body) {
  const heading = title ? `<h2 class="card-title">${esc(title)}</h2>` : "";
  return `<div class="card">${heading}${body}</div>`;
}

// ---- 從畫面讀值 ----

export const textValue = (root, id) => root.querySelector(`#${id}`).value.trim();
export const checkedValue = (root, id) => root.querySelector(`#${id}`).checked;

function bindReveal(root) {
  for (const btn of root.querySelectorAll("button.reveal")) {
    const input = root.querySelector(`#${btn.dataset.target}`);
    btn.addEventListener("click", () => {
      const show = input.type === "password";
      input.type = show ? "text" : "password";
      btn.setAttribute("aria-pressed", String(show));
      btn.title = show ? "隱藏金鑰" : "顯示金鑰";
      btn.setAttribute("aria-label", btn.title);
    });
  }
}

/**
 * 建立一個設定頁。
 * - render(cfg)：回傳表單內容的 HTML
 * - read(section)：從畫面讀出「這一頁負責的欄位」，回傳物件（存檔時只送這些欄位）
 * - bind(section, ctx)：（選填）綁定這一頁額外的互動
 * - savedNote(before, after)：（選填）存檔成功後附加在「已儲存」後面的說明
 */
export function makePage({ title, description, render, read, bind, savedNote }) {
  let section = null;
  let ctx = null;
  let saved = null;

  const statusEl = () => section.querySelector(".status-text");

  function setStatus(text, isError = false) {
    const el = statusEl();
    el.textContent = text;
    el.classList.toggle("error", isError);
  }

  async function load(sec, context) {
    section = sec;
    ctx = context;
    const cfg = await api.getConfig();
    section.innerHTML = `
      <h1 class="page-title">${esc(title)}</h1>
      <p class="page-desc">${esc(description)}</p>
      <form novalidate>
        ${render(cfg)}
        <div class="actions">
          <button type="submit">儲存</button>
          <span class="status-text" role="status"></span>
        </div>
      </form>`;
    bindReveal(section);
    bind?.(section, ctx, setStatus);
    saved = read(section);
    section.querySelector("form").addEventListener("submit", onSubmit);
  }

  async function onSubmit(ev) {
    ev.preventDefault();
    setStatus("儲存中…");
    const before = saved;
    const values = read(section);
    try {
      await api.saveFields(values);
    } catch (err) {
      // 失敗時畫面維持使用者輸入的內容（仍算未存修改），只顯示原因。
      setStatus(`儲存失敗：${err}`, true);
      return;
    }
    await load(section, ctx);
    setStatus(`已儲存${savedNote?.(before, saved) ?? ""}`);
    ctx.refreshStatus();
  }

  function isDirty() {
    if (!section || !saved) return false;
    return JSON.stringify(read(section)) !== JSON.stringify(saved);
  }

  return { load, isDirty };
}
