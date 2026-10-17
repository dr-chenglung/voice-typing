// 設定視窗：讀取/儲存 config.toml（經由 Rust 端 get_config/save_config command）。
// API key 由 get_config 明文取回並填入欄位，預設以圓點遮住，按眼睛按鈕切換顯示；
// save_config 以欄位內容為準（留空＝清除金鑰）。

const { core } = window.__TAURI__;

const form = document.getElementById("form");
const sttApiKeyInput = document.getElementById("stt_api_key");
const sttApiUrlInput = document.getElementById("stt_api_url");
const sttModelInput = document.getElementById("stt_model");
const llmApiKeyInput = document.getElementById("llm_api_key");
const llmApiUrlInput = document.getElementById("llm_api_url");
const llmModelInput = document.getElementById("llm_model");
const vocabularyInput = document.getElementById("vocabulary");
const enableFormattingInput = document.getElementById("enable_formatting");
const enableCorrectionInput = document.getElementById("enable_correction");
const hotkeySelect = document.getElementById("hotkey");
const targetLanguageSelect = document.getElementById("target_language_select");
const targetLanguageCustomRow = document.getElementById("target_language_custom_row");
const targetLanguageCustomInput = document.getElementById("target_language_custom");
const translateModeActiveInput = document.getElementById("translate_mode_active");
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
  sttApiKeyInput.value = cfg.stt_api_key;
  sttApiUrlInput.value = cfg.stt_api_url;
  sttModelInput.value = cfg.stt_model;
  llmApiKeyInput.value = cfg.llm_api_key;
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
  translateModeActiveInput.checked = cfg.translate_mode_active;
  hotkeySelect.value = cfg.hotkey;
  originalHotkey = cfg.hotkey;
}

form.addEventListener("submit", async (ev) => {
  ev.preventDefault();
  statusEl.textContent = "儲存中…";
  try {
    await core.invoke("save_config", {
      update: {
        stt_api_key: sttApiKeyInput.value.trim(),
        stt_api_url: sttApiUrlInput.value.trim(),
        stt_model: sttModelInput.value.trim(),
        llm_api_key: llmApiKeyInput.value.trim(),
        llm_api_url: llmApiUrlInput.value.trim(),
        llm_model: llmModelInput.value.trim(),
        vocabulary: vocabularyInput.value.trim(),
        enable_formatting: enableFormattingInput.checked,
        enable_correction: enableCorrectionInput.checked,
        hotkey: hotkeySelect.value,
        target_language: resolvedTargetLanguage(),
        translate_mode_active: translateModeActiveInput.checked,
      },
    });
    const hotkeyChanged = hotkeySelect.value !== originalHotkey;
    const restartNote = hotkeyChanged ? "（熱鍵已變更，需重啟程式才生效）" : "";
    statusEl.textContent = `已儲存 ${restartNote}`;
    await load();
  } catch (err) {
    statusEl.textContent = `儲存失敗：${err}`;
  }
});

// 眼睛按鈕：切換金鑰欄位的遮罩（password ↔ text）。
for (const btn of document.querySelectorAll("button.reveal")) {
  const input = document.getElementById(btn.dataset.target);
  btn.addEventListener("click", () => {
    const show = input.type === "password";
    input.type = show ? "text" : "password";
    btn.setAttribute("aria-pressed", String(show));
    btn.title = show ? "隱藏金鑰" : "顯示金鑰";
    btn.setAttribute("aria-label", btn.title);
  });
}

load();
