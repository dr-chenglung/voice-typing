// 主視窗進入點：產生側邊欄與各頁容器、處理頁面切換、維護狀態列。
// 所有切頁（側邊欄點擊、托盤選單、頁面內的連結）都走同一個 router.go()，
// 所以「有未儲存的修改要先確認」只在 lib/router.js 裡做一次。

import { PAGES, parsePage, createNavigator } from "./lib/router.js";
import * as api from "./api.js";

const STORAGE_KEY = "voicetyping.page";
const STATE_LABELS = { idle: "閒置", recording: "錄音中", processing: "處理中" };

const content = document.getElementById("content");
const navMain = document.getElementById("nav-main");
const navFooter = document.getElementById("nav-footer");
const statusDot = document.getElementById("status-dot");
const statusText = document.getElementById("status-text");
const confirmDialog = document.getElementById("confirm-dialog");
const confirmMessage = document.getElementById("confirm-message");
const confirmOk = document.getElementById("confirm-ok");
const confirmCancel = document.getElementById("confirm-cancel");

// localStorage 在某些情況下會讀寫失敗，一律包起來，讀不到就當作沒有。
function storedPage() {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

function rememberPage(id) {
  try {
    localStorage.setItem(STORAGE_KEY, id);
  } catch {
    /* 記不住頁面不影響使用 */
  }
}

// ---- 頁內確認對話框（不使用 window.confirm） ----
function askConfirm(message) {
  if (!confirmDialog.hidden) return Promise.resolve(false);
  return new Promise((resolve) => {
    confirmMessage.textContent = message;
    confirmDialog.hidden = false;
    confirmOk.focus();
    const finish = (value) => {
      confirmDialog.hidden = true;
      confirmOk.removeEventListener("click", onOk);
      confirmCancel.removeEventListener("click", onCancel);
      document.removeEventListener("keydown", onKey);
      resolve(value);
    };
    const onOk = () => finish(true);
    const onCancel = () => finish(false);
    const onKey = (ev) => {
      if (ev.key === "Escape") finish(false);
    };
    confirmOk.addEventListener("click", onOk);
    confirmCancel.addEventListener("click", onCancel);
    document.addEventListener("keydown", onKey);
  });
}

// ---- 狀態列 ----
let currentState = "idle";
let modeText = "一般模式";

function renderStatusBar() {
  statusDot.className = `state-${currentState}`;
  statusText.textContent = `${STATE_LABELS[currentState] ?? "閒置"} · ${modeText}`;
}

async function refreshStatusBar() {
  const cfg = await api.getConfig();
  modeText = cfg.translate_mode_active ? `翻譯模式 → ${cfg.target_language}` : "一般模式";
  renderStatusBar();
}

// ---- 頁面切換 ----
let currentModule = null;

const ctx = {
  navigate: (id) => router.go(id),
  refreshStatus: () => refreshStatusBar().catch(() => {}),
  confirm: (message) => askConfirm(message),
};

async function showPage(id) {
  const section = document.getElementById(`page-${id}`);
  let module = null;
  try {
    module = await import(`./pages/${id}.js`);
    await module.load(section, ctx);
  } catch (err) {
    // 載入失敗也要讓使用者看到原因，而不是一片空白。
    section.textContent = `載入失敗：${err}`;
    module = null;
  }
  currentModule = module;
  for (const page of PAGES) {
    document.getElementById(`page-${page.id}`).hidden = page.id !== id;
  }
  for (const item of document.querySelectorAll(".nav-item")) {
    item.classList.toggle("active", item.dataset.page === id);
  }
  rememberPage(id);
}

const router = createNavigator({
  isDirty: () => currentModule?.isDirty?.() ?? false,
  askConfirm: () => askConfirm("這一頁有尚未儲存的變更，確定要離開嗎？"),
  show: showPage,
});

function buildShell() {
  for (const page of PAGES) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "nav-item";
    button.dataset.page = page.id;
    button.textContent = page.label;
    button.addEventListener("click", () => router.go(page.id));
    (page.group === "footer" ? navFooter : navMain).appendChild(button);

    const section = document.createElement("section");
    section.id = `page-${page.id}`;
    section.className = "page";
    section.hidden = true;
    content.appendChild(section);
  }
}

async function init() {
  buildShell();
  try {
    currentState = (await api.getStatus()).state;
  } catch {
    /* 取不到就維持閒置 */
  }
  try {
    await refreshStatusBar();
  } catch {
    renderStatusBar();
  }
  await api.listen("app-status", (ev) => {
    currentState = ev.payload.state;
    renderStatusBar();
  });
  await api.listen("navigate", (ev) => router.go(String(ev.payload)));

  // Rust 開啟視窗時若指定了頁面，會注入 window.__INITIAL_PAGE__，優先於網址 hash。
  const initial = window.__INITIAL_PAGE__ ? `#${window.__INITIAL_PAGE__}` : location.hash;
  await router.go(parsePage(initial, storedPage()));
}

init();
