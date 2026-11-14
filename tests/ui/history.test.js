import { test } from "node:test";
import assert from "node:assert/strict";
import { filterHistory, latestEntry } from "../../ui/app/lib/history.js";

const entries = [
  { timestamp: 2, text: "Hello World", translated: true, target_language: "English", source_text: "你好世界" },
  { timestamp: 1, text: "今天開會", translated: false, target_language: "", source_text: "" },
];

test("filterHistory 空查詢回原陣列", () => assert.equal(filterHistory(entries, "  ").length, 2));

test("filterHistory 不分大小寫比對譯文", () =>
  assert.deepEqual(filterHistory(entries, "hello").map((e) => e.timestamp), [2]));

test("filterHistory 也比對翻譯前原文", () =>
  assert.deepEqual(filterHistory(entries, "世界").map((e) => e.timestamp), [2]));

test("filterHistory 沒有符合時回空陣列", () => assert.deepEqual(filterHistory(entries, "不存在"), []));

test("latestEntry 取第一筆，空陣列回 null", () => {
  assert.equal(latestEntry(entries).timestamp, 2);
  assert.equal(latestEntry([]), null);
});
