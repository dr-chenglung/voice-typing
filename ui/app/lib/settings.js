// 設定欄位與合併邏輯（純函式）。

/** Rust 端 `ConfigUpdate` 的 12 個欄位（commands.rs），save_config 要求全部都給。 */
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
 * 整理風格。第一階段只有內建的兩種，對應舊的 enable_formatting 開關：
 * 基本型＝關閉智慧排版，積極型＝開啟。自訂風格留到第三階段。
 */
export const STYLES = [
  {
    id: "builtin-basic",
    name: "基本型",
    description: "輕度清理：去贅詞、補標點、修正錯字，不分段、不條列。",
  },
  {
    id: "builtin-active",
    name: "積極型",
    description: "基本型加上智慧排版：長內容會分段、條列，允許輕度濃縮。",
  },
];

export function styleIdFromFormatting(enabled) {
  return enabled ? "builtin-active" : "builtin-basic";
}

export function formattingFromStyleId(id) {
  return id === "builtin-active";
}
