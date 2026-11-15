import { test } from "node:test";
import assert from "node:assert/strict";
import {
  UPDATE_KEYS,
  mergeUpdate,
  PRESET_LANGUAGES,
  languageSelection,
  resolveTargetLanguage,
  STYLE_CHOICES,
  styleChoiceFromConfig,
  configPatchForStyle,
  endpointHost,
  setupWarnings,
  hotkeyLabel,
} from "../../ui/app/lib/settings.js";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const base = {
  stt_api_key: "k1", stt_api_url: "u", stt_model: "m", llm_api_key: "k2",
  llm_api_url: "u2", llm_model: "m2", enable_correction: true, hotkey: "right_alt",
  vocabulary: "Tauri", enable_formatting: false, use_custom_prompt: false, target_language: "English", translate_mode_active: false,
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

test("UPDATE_KEYS 恰為 13 個欄位", () => assert.equal(UPDATE_KEYS.length, 13));

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

test("hotkeyLabel 顯示中文名稱，未知的熱鍵原樣顯示", () => {
  assert.equal(hotkeyLabel("right_alt"), "右 Alt");
  assert.equal(hotkeyLabel("right_ctrl"), "右 Ctrl");
  assert.equal(hotkeyLabel("caps_lock"), "caps_lock");
});

test("UPDATE_KEYS 的名稱與 Rust 的 ConfigUpdate 欄位完全一致", () => {
  const rust = readFileSync(fileURLToPath(new URL("../../src-tauri/src/commands.rs", import.meta.url)), "utf8");
  const body = rust.match(/pub struct ConfigUpdate \{([\s\S]*?)\n\}/)[1];
  const fields = [...body.matchAll(/pub (\w+):/g)].map((m) => m[1]);
  assert.deepEqual([...fields].sort(), [...UPDATE_KEYS].sort());
});

test("STYLE_CHOICES 順序與名稱固定", () => {
  assert.deepEqual(STYLE_CHOICES.map((c) => c.id), ["none", "basic", "active", "custom"]);
  assert.deepEqual(STYLE_CHOICES.map((c) => c.name), ["不使用 AI 整理", "基本型", "積極型", "自訂"]);
});

test("styleChoiceFromConfig 四種組合", () => {
  assert.equal(styleChoiceFromConfig({ enable_correction: false, enable_formatting: true, use_custom_prompt: true }), "none");
  assert.equal(styleChoiceFromConfig({ enable_correction: true, enable_formatting: true, use_custom_prompt: true }), "custom");
  assert.equal(styleChoiceFromConfig({ enable_correction: true, enable_formatting: true, use_custom_prompt: false }), "active");
  assert.equal(styleChoiceFromConfig({ enable_correction: true, enable_formatting: false, use_custom_prompt: false }), "basic");
});

test("configPatchForStyle 與 styleChoiceFromConfig 互為反函式", () => {
  for (const { id } of STYLE_CHOICES) assert.equal(styleChoiceFromConfig(configPatchForStyle(id)), id);
});

test("只有積極型會開啟 enable_formatting（翻譯模式的排版跟著它）", () => {
  const withFormatting = STYLE_CHOICES.filter(({ id }) => configPatchForStyle(id).enable_formatting).map((s) => s.id);
  assert.deepEqual(withFormatting, ["active"]);
});

test("configPatchForStyle：每種風格寫入的三個欄位", () => {
  assert.deepEqual(configPatchForStyle("none"), { enable_correction: false, enable_formatting: false, use_custom_prompt: false });
  assert.deepEqual(configPatchForStyle("basic"), { enable_correction: true, enable_formatting: false, use_custom_prompt: false });
  assert.deepEqual(configPatchForStyle("active"), { enable_correction: true, enable_formatting: true, use_custom_prompt: false });
  assert.deepEqual(configPatchForStyle("custom"), { enable_correction: true, enable_formatting: false, use_custom_prompt: true });
});

test("endpointHost 取出主機（含連接埠），不合法的網址回空字串", () => {
  assert.equal(endpointHost("https://api.groq.com/openai/v1/chat/completions"), "api.groq.com");
  assert.equal(endpointHost("http://127.0.0.1:8000/v1/x"), "127.0.0.1:8000");
  assert.equal(endpointHost("not a url"), "");
  assert.equal(endpointHost(""), "");
});

const filled = { stt_api_key: "a", llm_api_key: "b", enable_correction: true, translate_mode_active: false };

test("setupWarnings：金鑰都有填就沒有提醒", () => assert.deepEqual(setupWarnings(filled), []));

test("setupWarnings：語音辨識金鑰空白（含只有空白字元）", () => {
  assert.deepEqual(setupWarnings({ ...filled, stt_api_key: "  " }),
    [{ message: "語音辨識 API 還沒有填金鑰", goTo: "stt" }]);
});

test("setupWarnings：整理用金鑰只在會用到時才檢查", () => {
  const noLlmKey = { ...filled, llm_api_key: "" };
  assert.deepEqual(setupWarnings({ ...noLlmKey, enable_correction: true }),
    [{ message: "AI 整理 API 還沒有填金鑰", goTo: "llm" }]);
  assert.deepEqual(setupWarnings({ ...noLlmKey, enable_correction: false, translate_mode_active: false }), []);
  assert.deepEqual(setupWarnings({ ...noLlmKey, enable_correction: false, translate_mode_active: true }),
    [{ message: "AI 整理 API 還沒有填金鑰", goTo: "llm" }]);
});
