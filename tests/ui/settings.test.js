import { test } from "node:test";
import assert from "node:assert/strict";
import {
  UPDATE_KEYS,
  mergeUpdate,
  PRESET_LANGUAGES,
  languageSelection,
  resolveTargetLanguage,
  STYLES,
  styleIdFromFormatting,
  formattingFromStyleId,
} from "../../ui/app/lib/settings.js";

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

test("languageSelection 區分清單內與自訂語言", () => {
  assert.deepEqual(languageSelection("Japanese"), { select: "Japanese", custom: "" });
  assert.deepEqual(languageSelection("French"), { select: "__custom__", custom: "French" });
});

test("resolveTargetLanguage 自訂時取修剪後的輸入", () => {
  assert.equal(resolveTargetLanguage("__custom__", "  French "), "French");
  assert.equal(resolveTargetLanguage("Korean", "ignored"), "Korean");
  assert.equal(resolveTargetLanguage("__custom__", "   "), "");
});

test("PRESET_LANGUAGES 與舊設定視窗的清單相同", () => {
  assert.deepEqual(PRESET_LANGUAGES, [
    "English", "Japanese", "Korean", "Simplified Chinese",
    "Traditional Chinese", "Spanish", "Vietnamese",
  ]);
});

test("整理風格與 enable_formatting 互相對應", () => {
  assert.equal(styleIdFromFormatting(true), "builtin-active");
  assert.equal(styleIdFromFormatting(false), "builtin-basic");
  assert.equal(formattingFromStyleId("builtin-active"), true);
  assert.equal(formattingFromStyleId("builtin-basic"), false);
});

test("STYLES 有基本型與積極型兩項，id 固定", () => {
  assert.deepEqual(STYLES.map((s) => s.id), ["builtin-basic", "builtin-active"]);
});
