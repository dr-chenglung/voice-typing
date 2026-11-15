// 錄音膠囊浮動視窗。
// 後端 overlay.rs 用 emit_to(<overlay 視窗 label>, ...) 送三種事件：
//   - "overlay-show"：每次顯示時送一次，payload { translate, seq, elapsed_ms }。
//       重置波形、切換一般／翻譯配色、記下秒數起點；收到後在下一影格回報 "overlay-alive"（看門狗）。
//       elapsed_ms 在視窗失效重建時為「已經錄了多久」，秒數才不會歸零。
//   - "meter"：錄音中每 ~33ms 一次，payload { bars: 24 個原始 RMS, warning: "none"|"low"|"muted" }。
//   - "overlay-processing"：錄音結束、等待 STT／LLM 回傳，切成不確定進度條。
// 波形的校準與平滑在 lib/bars.js；這裡只負責把狀態畫到 DOM 上。

import { BAR_COUNT, createBarState, updateBars, formatElapsed } from "./lib/bars.js";

const WARNING_TEXT = { low: "聲音太小", muted: "收不到聲音" };
const MIN_SCALE = 3 / 18; // 條高 3~18px，固定高度 18px 用 scaleY 縮放

const pill = document.getElementById("pill");
const barsEl = document.getElementById("bars");
const timerEl = document.getElementById("timer");
const warningEl = document.getElementById("warning");
const warningTextEl = document.getElementById("warning-text");

const bars = Array.from({ length: BAR_COUNT }, () => barsEl.appendChild(document.createElement("span")));

let barState = createBarState();
let heights = new Array(BAR_COUNT).fill(0);
let startedAt = null; // performance.now() 基準的錄音起點；null = 目前不在錄音
let shownTimer = "";
let shownWarning = "none";

function setWarning(kind) {
  if (kind === shownWarning) return;
  shownWarning = kind;
  if (kind === "low" || kind === "muted") {
    warningEl.dataset.tone = kind;
    warningTextEl.textContent = WARNING_TEXT[kind];
    warningEl.hidden = false;
  } else {
    warningEl.hidden = true;
  }
}

function resetForShow(translate) {
  barState = createBarState();
  heights = new Array(BAR_COUNT).fill(0);
  shownTimer = "";
  document.body.classList.remove("processing");
  document.body.classList.toggle("translate", Boolean(translate));
  setWarning("none");
  // 重新播放膠囊的淡入。
  pill.classList.remove("enter");
  void pill.offsetWidth;
  pill.classList.add("enter");
}

function render() {
  for (let i = 0; i < BAR_COUNT; i++) {
    const h = heights[i];
    bars[i].style.transform = `scaleY(${(MIN_SCALE + h * (1 - MIN_SCALE)).toFixed(3)})`;
    bars[i].style.opacity = (0.7 + h * 0.3).toFixed(2);
  }
  if (startedAt !== null) {
    const t = formatElapsed(performance.now() - startedAt);
    if (t !== shownTimer) {
      shownTimer = t;
      timerEl.textContent = t;
    }
  }
  requestAnimationFrame(render);
}

const { event } = window.__TAURI__;

event.listen("overlay-show", (e) => {
  const p = e.payload ?? {};
  resetForShow(p.translate);
  startedAt = performance.now() - (Number(p.elapsed_ms) || 0);
  // 看門狗回報：在下一個動畫影格把序號送回後端，證明 JS 與繪製迴圈都還在跑；
  // 後端沒在時限內收到就會重建這個視窗（見 overlay.rs 檔頭說明）。
  if (p.seq != null) requestAnimationFrame(() => event.emit("overlay-alive", p.seq));
});

event.listen("meter", (e) => {
  if (document.body.classList.contains("processing")) return;
  const p = e.payload ?? {};
  if (Array.isArray(p.bars) && p.bars.length === BAR_COUNT) heights = updateBars(barState, p.bars);
  setWarning(p.warning ?? "none");
});

event.listen("overlay-processing", () => {
  document.body.classList.add("processing");
  startedAt = null;
  setWarning("none");
});

requestAnimationFrame(render);
