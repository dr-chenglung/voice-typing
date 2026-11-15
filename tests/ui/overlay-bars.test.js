import { test } from "node:test";
import assert from "node:assert/strict";
import { BAR_COUNT, createBarState, updateBars, formatElapsed } from "../../ui/overlay/lib/bars.js";

const feed = (s, raw, times) => {
  let h;
  for (let i = 0; i < times; i++) h = updateBars(s, raw);
  return h;
};
const uniform = (v) => Array(BAR_COUNT).fill(v);

test("輸出長度固定且落在 0~1", () => {
  const s = createBarState();
  for (const v of [0, 0.0001, 0.01, 0.5, 2]) {
    const h = updateBars(s, uniform(v));
    assert.equal(h.length, BAR_COUNT);
    assert.ok(h.every((x) => x >= 0 && x <= 1), `v=${v}`);
  }
});

test("全零輸入維持全平", () => assert.ok(feed(createBarState(), uniform(0), 30).every((x) => x === 0)));

test("穩定的背景音量會被校準成幾乎平的", () =>
  assert.ok(feed(createBarState(), uniform(0.003), 60).every((x) => x < 0.05)));

test("大小聲交替時講話的那一下明顯升高，小聲音量與大聲音量都適用", () => {
  for (const base of [0.002, 0.05]) {
    const s = createBarState();
    for (let i = 0; i < 30; i++) updateBars(s, uniform(base));
    const h = feed(s, uniform(base * 8), 3);
    assert.ok(Math.max(...h) > 0.5, `base=${base}`);
  }
});

test("上升快、下降慢", () => {
  const s = createBarState();
  for (let i = 0; i < 30; i++) updateBars(s, uniform(0.003));
  const up = updateBars(s, uniform(0.03));
  assert.ok(Math.max(...up) >= 0.4);
  const peak = Math.max(...feed(s, uniform(0.03), 3));
  const down = Math.max(...updateBars(s, uniform(0.003)));
  assert.ok(down > peak * 0.6);
});

test("相同輸入時中間的條不低於兩端", () => {
  const s = createBarState();
  for (let i = 0; i < 30; i++) updateBars(s, uniform(0.003));
  const h = feed(s, uniform(0.03), 5);
  const mid = Math.floor(BAR_COUNT / 2); // 奇數根時就是正中間那一根
  assert.ok(h[mid] >= h[0] && h[mid] >= h[BAR_COUNT - 1]);
});

test("formatElapsed 為 m:ss", () => {
  assert.equal(formatElapsed(0), "0:00");
  assert.equal(formatElapsed(7_400), "0:07");
  assert.equal(formatElapsed(83_000), "1:23");
  assert.equal(formatElapsed(3_600_000), "60:00");
  assert.equal(formatElapsed(-50), "0:00");
});
