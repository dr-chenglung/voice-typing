// 歷史紀錄頁：搜尋、逐筆複製、顯示翻譯前的原文、清除全部。
import * as api from "../api.js";
import { escapeHtml as esc } from "../lib/html.js";
import { filterHistory } from "../lib/history.js";

let section = null;
let ctx = null;
let listening = false;
let entries = [];
let shown = [];
let query = "";

function formatTimestamp(unixSeconds) {
  return new Date(unixSeconds * 1000).toLocaleString();
}

function itemHtml(entry, index) {
  const badge = entry.translated
    ? ` <span class="badge">翻譯 → ${esc(entry.target_language)}</span>`
    : "";
  const source =
    entry.translated && entry.source_text
      ? `<button type="button" class="link" data-action="toggle">顯示原文</button>
         <div class="history-source" hidden>${esc(entry.source_text)}</div>`
      : "";
  return `<li class="history-item">
    <div class="history-meta"><span>${esc(formatTimestamp(entry.timestamp))}${badge}</span>
      <button type="button" class="secondary small" data-action="copy" data-i="${index}">複製</button></div>
    <div class="history-text">${esc(entry.text)}</div>
    ${source}
  </li>`;
}

function renderList() {
  shown = filterHistory(entries, query);
  const empty = section.querySelector("#history_empty");
  section.querySelector("#history_list").innerHTML = shown.map(itemHtml).join("");
  empty.hidden = shown.length > 0;
  empty.textContent = entries.length === 0 ? "尚無歷史紀錄" : "沒有符合的紀錄";
  section.querySelector("#history_clear").disabled = entries.length === 0;
}

async function refresh() {
  entries = await api.getHistory();
  renderList();
}

async function onListClick(ev) {
  const btn = ev.target.closest("button[data-action]");
  if (!btn) return;
  if (btn.dataset.action === "copy") {
    const status = section.querySelector(".status-text");
    try {
      await navigator.clipboard.writeText(shown[Number(btn.dataset.i)].text);
      status.textContent = "已複製";
      status.classList.remove("error");
    } catch (err) {
      status.textContent = `複製失敗：${err}`;
      status.classList.add("error");
    }
  } else if (btn.dataset.action === "toggle") {
    const source = btn.nextElementSibling;
    source.hidden = !source.hidden;
    btn.textContent = source.hidden ? "顯示原文" : "隱藏原文";
  }
}

export async function load(sec, context) {
  section = sec;
  ctx = context;
  query = "";
  section.innerHTML = `
    <h1 class="page-title">歷史紀錄</h1>
    <p class="page-desc">過去輸出的文字，最多保留 500 筆。</p>
    <div class="toolbar">
      <input type="text" id="history_search" placeholder="搜尋文字…" spellcheck="false" />
      <button type="button" class="secondary" id="history_clear">清除全部</button>
      <span class="status-text" role="status"></span>
    </div>
    <ul id="history_list" class="history-list"></ul>
    <p id="history_empty" class="hint" hidden></p>`;
  section.querySelector("#history_search").addEventListener("input", (ev) => {
    query = ev.target.value;
    renderList();
  });
  section.querySelector("#history_list").addEventListener("click", onListClick);
  section.querySelector("#history_clear").addEventListener("click", async () => {
    if (!(await ctx.confirm("確定要清除全部歷史紀錄嗎？此動作無法復原。"))) return;
    const status = section.querySelector(".status-text");
    status.textContent = "";
    status.classList.remove("error");
    try {
      await api.clearHistory();
      await refresh();
    } catch (err) {
      // 失敗時清單維持原樣，並讓使用者看到原因。
      status.textContent = `清除失敗：${err}`;
      status.classList.add("error");
    }
  });
  await refresh();
  if (!listening) {
    listening = true;
    const onChange = () => {
      if (section && !section.hidden) refresh().catch(() => {});
    };
    await api.listenSafely("history-updated", onChange);
    await api.listenSafely("history-cleared", onChange);
  }
}

export function isDirty() {
  return false;
}
