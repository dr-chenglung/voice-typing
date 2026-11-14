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
