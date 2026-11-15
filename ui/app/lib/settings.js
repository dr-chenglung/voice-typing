// 設定欄位與合併邏輯（純函式）。

/** Rust 端 `ConfigUpdate` 的 13 個欄位（commands.rs），save_config 要求全部都給。 */
export const UPDATE_KEYS = [
  "stt_api_key",
  "stt_api_url",
  "stt_model",
  "llm_api_key",
  "llm_api_url",
  "llm_model",
  "enable_correction",
  "hotkey",
  "vocabulary",
  "enable_formatting",
  "use_custom_prompt",
  "target_language",
  "translate_mode_active",
];

/**
 * 以 current（剛從 Rust 讀到的完整設定）為底，只覆蓋 partial 給的欄位。
 * 每頁存檔時只送自己的欄位，其餘欄位取存檔當下重新讀到的值，
 * 所以 A 頁存檔不會把 B 頁已經存過的值蓋回舊的。
 * 不屬於 ConfigUpdate 的欄位一律丟掉。
 */
export function mergeUpdate(current, partial) {
  const out = {};
  for (const key of UPDATE_KEYS) {
    out[key] = Object.hasOwn(partial, key) ? partial[key] : current[key];
  }
  return out;
}

/** 翻譯目標語言的常用選項（英文語言名稱，直接送進翻譯提示詞），與舊設定視窗的清單相同。 */
export const PRESET_LANGUAGES = [
  "English",
  "Japanese",
  "Korean",
  "Simplified Chinese",
  "Traditional Chinese",
  "Spanish",
  "Vietnamese",
];

/** 把存檔裡的目標語言換算成下拉選單要選哪一項；不在常用清單內就選「自訂」。 */
export function languageSelection(target) {
  return PRESET_LANGUAGES.includes(target)
    ? { select: target, custom: "" }
    : { select: "__custom__", custom: target };
}

/** 下拉選單與自訂輸入框換算回要存的語言；選「自訂」時取修剪後的輸入。 */
export function resolveTargetLanguage(select, custom) {
  return select === "__custom__" ? custom.trim() : select;
}

/**
 * AI 整理風格的四個選項，全部由既有欄位推導，不另存「目前風格」：
 * - enable_correction：是否交給語言模型整理（關閉＝不使用 AI 整理）
 * - enable_formatting：積極型（翻譯模式的智慧排版也跟著它，所以只有積極型為 true）
 * - use_custom_prompt：自訂型（提示詞文字另存於 custom_prompt.txt）
 */
export const STYLE_CHOICES = [
  { id: "none", name: "不使用 AI 整理", description: "直接輸出辨識原文，不交給語言模型。" },
  { id: "basic", name: "基本型", description: "輕度校正：去贅詞、補標點、修正錯字，不分段、不條列。" },
  { id: "active", name: "積極型", description: "基本型加上智慧排版：長內容會分段、條列，允許輕度濃縮。" },
  { id: "custom", name: "自訂", description: "使用你自己寫的提示詞。" },
];

/** 由設定推導目前選的是哪一種風格。 */
export function styleChoiceFromConfig(cfg) {
  if (!cfg.enable_correction) return "none";
  if (cfg.use_custom_prompt) return "custom";
  return cfg.enable_formatting ? "active" : "basic";
}

/** 選某種風格時要寫入的三個欄位（與 styleChoiceFromConfig 互為反函式）。 */
export function configPatchForStyle(choice) {
  return {
    enable_correction: choice !== "none",
    enable_formatting: choice === "active",
    use_custom_prompt: choice === "custom",
  };
}

/** 服務網址的主機名稱（含連接埠），給主頁顯示用；網址不合法就回空字串。 */
export function endpointHost(url) {
  try {
    return new URL(url).host;
  } catch {
    return "";
  }
}

/**
 * 主頁的「設定提醒」：目前程式規定語音辨識一定要有金鑰（本機服務也要隨便填一個字）；
 * 整理用的金鑰只有在會用到時（有啟用整理，或翻譯模式開著）才檢查。
 */
export function setupWarnings(cfg) {
  const warnings = [];
  if (!String(cfg.stt_api_key ?? "").trim()) {
    warnings.push({ message: "語音辨識 API 還沒有填金鑰", goTo: "stt" });
  }
  const llmInUse = cfg.enable_correction || cfg.translate_mode_active;
  if (llmInUse && !String(cfg.llm_api_key ?? "").trim()) {
    warnings.push({ message: "AI 整理 API 還沒有填金鑰", goTo: "llm" });
  }
  return warnings;
}

/** 與 Rust 端 `validate_custom_prompt` 的錯誤訊息相同（有測試比對），選自訂但文字空白時顯示。 */
export const EMPTY_CUSTOM_PROMPT_MESSAGE = "自訂提示詞還是空的，請先寫內容，或按「從基本型複製」";

/**
 * 選「自訂」之前的決策。文字框空白時一律回錯誤、**不先存檔**：否則會把原本存好的提示詞蓋成空的，
 * 選項卻又切換失敗。文字有改過才需要先存（選了卻讀不到未存的內容是更糟的結果）。
 */
export function planCustomSelection(customText, savedCustom) {
  if (!String(customText).trim()) return { error: EMPTY_CUSTOM_PROMPT_MESSAGE, saveFirst: false };
  return { error: null, saveFirst: customText !== savedCustom };
}

const HOTKEY_LABELS = { right_alt: "右 Alt", right_ctrl: "右 Ctrl" };

/** 熱鍵設定值換成給人看的名稱；不認得的值原樣顯示。 */
export function hotkeyLabel(id) {
  return HOTKEY_LABELS[id] ?? id;
}
