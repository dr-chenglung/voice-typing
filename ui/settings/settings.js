// 設定視窗：讀取/儲存 config.toml（經由 Rust 端 get_config/save_config command）。
// API key 絕不從後端明文取回：get_config 只回 has_api_key 布林值，
// save_config 的 api_key 欄位留空字串代表「不變更現有金鑰」。

const { core } = window.__TAURI__;

const form = document.getElementById("form");
const sttApiKeyInput = document.getElementById("stt_api_key");
const sttApiKeyHint = document.getElementById("stt_api_key_hint");
const sttApiUrlInput = document.getElementById("stt_api_url");
const sttModelInput = document.getElementById("stt_model");
const llmApiKeyInput = document.getElementById("llm_api_key");
const llmApiKeyHint = document.getElementById("llm_api_key_hint");
const llmApiUrlInput = document.getElementById("llm_api_url");
const llmModelInput = document.getElementById("llm_model");
const vocabularyInput = document.getElementById("vocabulary");
const enableFormattingInput = document.getElementById("enable_formatting");
const enableCorrectionInput = document.getElementById("enable_correction");
const hotkeySelect = document.getElementById("hotkey");
const targetLanguageSelect = document.getElementById("target_language_select");
const targetLanguageCustomRow = document.getElementById("target_language_custom_row");
const targetLanguageCustomInput = document.getElementById("target_language_custom");
const translateHotkeySelect = document.getElementById("translate_hotkey");
const statusEl = document.getElementById("status");

const PRESET_LANGUAGES = [
  "English",
  "Japanese",
  "Korean",
  "Simplified Chinese",
  "Traditional Chinese",
  "Spanish",
  "Vietnamese",
];

function syncTargetLanguageCustomRow() {
  targetLanguageCustomRow.classList.toggle(
    "hidden",
    targetLanguageSelect.value !== "__custom__"
  );
}

function resolvedTargetLanguage() {
  return targetLanguageSelect.value === "__custom__"
    ? targetLanguageCustomInput.value.trim()
    : targetLanguageSelect.value;
}

targetLanguageSelect.addEventListener("change", syncTargetLanguageCustomRow);

let originalHotkey = "";

async function load() {
  const cfg = await core.invoke("get_config");
  sttApiKeyHint.textContent = cfg.has_stt_api_key
    ? "目前已設定金鑰（留空儲存＝保留不變）"
    : "尚未設定金鑰";
  sttApiUrlInput.value = cfg.stt_api_url;
  sttModelInput.value = cfg.stt_model;
  llmApiKeyHint.textContent = cfg.has_llm_api_key
    ? "目前已設定金鑰（留空儲存＝保留不變）"
    : "尚未設定金鑰";
  llmApiUrlInput.value = cfg.llm_api_url;
  llmModelInput.value = cfg.llm_model;
  vocabularyInput.value = cfg.vocabulary;
  enableFormattingInput.checked = cfg.enable_formatting;
  enableCorrectionInput.checked = cfg.enable_correction;
  if (PRESET_LANGUAGES.includes(cfg.target_language)) {
    targetLanguageSelect.value = cfg.target_language;
    targetLanguageCustomInput.value = "";
  } else {
    targetLanguageSelect.value = "__custom__";
    targetLanguageCustomInput.value = cfg.target_language;
  }
  syncTargetLanguageCustomRow();
  translateHotkeySelect.value = cfg.translate_hotkey;
  hotkeySelect.value = cfg.hotkey;
  originalHotkey = cfg.hotkey;
}

form.addEventListener("submit", async (ev) => {
  ev.preventDefault();
  statusEl.textContent = "儲存中…";
  try {
    await core.invoke("save_config", {
      update: {
        stt_api_key: sttApiKeyInput.value,
        stt_api_url: sttApiUrlInput.value.trim(),
        stt_model: sttModelInput.value.trim(),
        llm_api_key: llmApiKeyInput.value,
        llm_api_url: llmApiUrlInput.value.trim(),
        llm_model: llmModelInput.value.trim(),
        vocabulary: vocabularyInput.value.trim(),
        enable_formatting: enableFormattingInput.checked,
        enable_correction: enableCorrectionInput.checked,
        hotkey: hotkeySelect.value,
        target_language: resolvedTargetLanguage(),
        translate_hotkey: translateHotkeySelect.value,
      },
    });
    sttApiKeyInput.value = "";
    llmApiKeyInput.value = "";
    const restartNote =
      hotkeySelect.value !== originalHotkey ? "（熱鍵已變更，需重啟程式才生效）" : "";
    statusEl.textContent = `已儲存 ${restartNote}`;
    await load();
  } catch (err) {
    statusEl.textContent = `儲存失敗：${err}`;
  }
});

load();
