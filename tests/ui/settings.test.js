import { test } from "node:test";
import assert from "node:assert/strict";
import { UPDATE_KEYS, mergeUpdate } from "../../ui/app/lib/settings.js";

const base = {
  stt_api_key: "k1", stt_api_url: "u", stt_model: "m", llm_api_key: "k2",
  llm_api_url: "u2", llm_model: "m2", enable_correction: true, hotkey: "right_alt",
  vocabulary: "Tauri", enable_formatting: false, target_language: "English", translate_mode_active: false,
};

test("mergeUpdate 只覆蓋 partial 給的欄位，其餘取 current", () => {
  const out = mergeUpdate(base, { target_language: "Japanese" });
  assert.equal(out.target_language, "Japanese");
  assert.equal(out.vocabulary, "Tauri");
  assert.equal(out.stt_api_key, "k1");
});

test("mergeUpdate 丟掉不屬於 ConfigUpdate 的欄位", () => {
  const out = mergeUpdate({ hotkey: "right_alt", extra: 1 }, { other: 2 });
  assert.equal("extra" in out, false);
  assert.equal("other" in out, false);
});

test("mergeUpdate 不修改傳入的 current", () => {
  const copy = { ...base };
  mergeUpdate(base, { hotkey: "right_ctrl" });
  assert.deepEqual(base, copy);
});

test("UPDATE_KEYS 恰為 12 個欄位", () => assert.equal(UPDATE_KEYS.length, 12));
