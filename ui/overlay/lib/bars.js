// 膠囊波形的純邏輯（不碰 DOM，方便 node --test）。
//
// 後端每 33ms 推來 20 個「原始 RMS」（未正規化）。麥克風輸入音量可能差很多
// （滑桿、距離、環境都會把整體乘一個倍率），固定增益無法兼顧，所以沿用舊版麥克風圖示的
// 「自動校準」：持續追蹤背景噪音底 floor 與近期峰值 ceil，把兩者之間映射到 0~1。
// 只有高於背景才會長高，講話的起伏自然形成跳動，不論輸入音量大小都適用。

export const BAR_COUNT = 20;

const ABS_SILENCE = 0.0005; // 絕對靜音保險絲：低於此值視為無聲
const DEAD = 0.1; // 死區：背景附近的小波動歸零，安靜時不抖動（2026-11-15 由 0.2 降到 0.1，讓小聲講話也看得出起伏）
const ATTACK = 0.55; // 往上追的速度（快）
const DECAY = 0.15; // 往下追的速度（慢）
const SNAP_ZERO = 0.005; // 低於此值直接歸零，避免殘留微小的尾巴
const CENTER_SHAPE = 0.3; // 兩端比中間最多矮 30%，像等化器的外形

export function createBarState(n = BAR_COUNT) {
  return {
    floor: 0.003,
    ceil: 0.03,
    seeded: false,
    heights: new Array(n).fill(0),
  };
}

/**
 * 吃進一批原始 RMS，回傳這一幀每根條的高度（0~1），並把平滑狀態寫回 state。
 * @param {ReturnType<typeof createBarState>} state
 * @param {number[]} raw 長度應為 state.heights.length
 */
export function updateBars(state, raw) {
  const n = state.heights.length;
  const peak = raw.reduce((m, v) => Math.max(m, v), 0);
  const c = (n - 1) / 2;

  let targets;
  if (peak < ABS_SILENCE) {
    targets = new Array(n).fill(0);
  } else {
    if (!state.seeded) {
      state.floor = peak;
      state.ceil = peak + 0.008;
      state.seeded = true;
    }
    // floor：低於它（背景變安靜或字詞間空檔）較快往下追；高於它（講話）只極慢上升，
    // 避免把語音本身吃進背景。ceil：瞬間追上新峰值，平時緩慢下降，且永遠高於 floor 一小段。
    state.floor += (peak - state.floor) * (peak < state.floor ? 0.1 : 0.01);
    state.ceil = peak > state.ceil ? peak : Math.max(state.floor + 0.006, state.ceil * 0.99);
    const span = Math.max(state.ceil - state.floor, 0.004);
    targets = raw.map((v, i) => {
      const norm = Math.min(1, Math.max(0, (v - state.floor) / span));
      const level = norm < DEAD ? 0 : (norm - DEAD) / (1 - DEAD);
      const shape = 1 - CENTER_SHAPE * (c > 0 ? Math.abs(i - c) / c : 0);
      return level * shape;
    });
  }

  for (let i = 0; i < n; i++) {
    const t = targets[i] ?? 0;
    const k = t > state.heights[i] ? ATTACK : DECAY;
    let h = state.heights[i] + (t - state.heights[i]) * k;
    if (h < SNAP_ZERO) h = 0;
    state.heights[i] = Math.min(1, Math.max(0, h));
  }
  return state.heights.slice();
}

/** 毫秒 → `m:ss`（不足一秒捨去，負數當 0）。 */
export function formatElapsed(ms) {
  const total = Math.max(0, Math.floor(ms / 1000));
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}
