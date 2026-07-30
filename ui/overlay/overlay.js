// 麥克風圖示疊加視窗。
// 後端 overlay.rs 用 emit_to("overlay", ...) 送兩種事件：
//   - "level"：f32 音量（已做 RMS + 衰減平滑），顯示時每 ~33ms 推一次
//   - "overlay-show"：每次顯示疊加視窗時送一次，用來重置動畫狀態
// 用 requestAnimationFrame 對音量做 easing，隨音量驅動：顏色（亮灰→活力橙）、發光、輕微縮放。
//
// 音量映射採「自動校準」：麥克風輸入音量可能差很多（從很小聲到開到最大），固定增益無法兼顧。
// 改為持續追蹤「背景噪音底 floor」與「近期峰值 ceiling」，把兩者之間的範圍映射到 0~1：
// 只有高於背景才會亮，講話的起伏自然形成脈動，且不論輸入音量大小都適用。

const ABS_SILENCE = 0.0005; // 絕對靜音保險絲：低於此值直接視為無聲
const EASE = 0.3;

const QUIET = [233, 233, 239]; // 安靜：亮灰
const LOUD = [255, 159, 28]; // 大聲：活力橙
const LOUD_TRANSLATE = [139, 140, 255]; // 翻譯模式：藍紫，與一般模式的活力橙區隔

const mic = document.getElementById("mic");
// 直接對每個 <path> 設 fill，避免依賴 <svg> 根元素的 fill 繼承。
const paths = Array.from(mic.querySelectorAll("path"));

let target = 0;
let displayed = 0;

// 自動校準狀態。
let floor = 0.003; // 背景噪音底（慢速追蹤）
let ceil = 0.03; // 近期峰值（快速追上、緩慢下降）
let seeded = false; // 每次開始錄音時，用第一筆音量重新定標，避免起始瞬間爆滿
let translateMode = false; // 本次顯示是否為翻譯模式，由 overlay-show 事件帶入

function lerp(a, b, t) {
  return Math.round(a + (b - a) * t);
}

// 把目前（已平滑的）音量 t∈[0,1] 套用到 SVG：顏色（亮灰→活力橙或藍紫）、發光、輕微放大。
function apply() {
  const t = displayed;
  const loud = translateMode ? LOUD_TRANSLATE : LOUD;
  const color = `rgb(${lerp(QUIET[0], loud[0], t)}, ${lerp(QUIET[1], loud[1], t)}, ${lerp(
    QUIET[2],
    loud[2],
    t
  )})`;
  for (const p of paths) p.style.fill = color;
  // 發光隨音量增強：glow 顏色隨模式切換（橙色或藍紫）疊加固定的黑色陰影，維持在深色底盤上的可讀性。
  const glow = translateMode ? "139, 140, 255" : "255, 159, 28";
  mic.style.filter = `drop-shadow(0 0 ${2 + t * 10}px rgba(${glow}, ${t * 0.9})) drop-shadow(0 1px 2px rgba(0, 0, 0, 0.5))`;
  mic.style.transform = `scale(${1 + t * 0.12})`;
}

function tick() {
  displayed += (target - displayed) * EASE;
  apply();
  requestAnimationFrame(tick);
}

function pushLevel(raw) {
  if (raw < ABS_SILENCE) {
    target = 0;
    return;
  }
  if (!seeded) {
    floor = raw;
    ceil = raw + 0.008;
    seeded = true;
  }
  // floor：低於它（背景變安靜或字詞間的空檔）就較快往下追；高於它（講話）只極慢上升，
  // 避免把語音本身吃進背景。實際效果是 floor ≈ 字詞之間的「谷底」音量。
  floor += (raw - floor) * (raw < floor ? 0.1 : 0.01);
  // ceil：瞬間追上新峰值，平時緩慢下降，並永遠高於 floor 一小段。
  ceil = raw > ceil ? raw : Math.max(floor + 0.006, ceil * 0.99);
  // 正規化成「背景之上的相對音量」。
  const span = Math.max(ceil - floor, 0.004);
  const norm = Math.min(1, Math.max(0, (raw - floor) / span));
  // 死區：背景附近的小波動歸零，安靜時就維持純亮灰、不抖動；超過門檻才重新拉回 0~1。
  const DEAD = 0.2;
  target = norm < DEAD ? 0 : (norm - DEAD) / (1 - DEAD);
}

function reset(translate) {
  target = 0;
  displayed = 0;
  seeded = false; // 下次講話重新自動定標
  translateMode = Boolean(translate);
}

apply();
requestAnimationFrame(tick);

const { event } = window.__TAURI__;
event.listen("level", (e) => pushLevel(e.payload));
event.listen("overlay-show", (e) => reset(e.payload?.translate));
