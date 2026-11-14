// 呼叫 Rust 端的薄包裝。`window.__TAURI__` 由 tauri.conf.json 的 withGlobalTauri 注入，
// 放在函式裡才讀取，確保模組載入時不會因為它還沒就緒而出錯。

import { mergeUpdate } from "./lib/settings.js";

const tauri = () => window.__TAURI__;

export const invoke = (cmd, args) => tauri().core.invoke(cmd, args);
export const listen = (name, handler) => tauri().event.listen(name, handler);

export const getConfig = () => invoke("get_config");

/**
 * 存檔：只送這一頁負責的欄位，其餘欄位取「存檔當下」重新讀到的設定。
 * 這樣 A 頁存檔不會把 B 頁已存過的值蓋回舊值（見 lib/settings.js 的 mergeUpdate）。
 */
export async function saveFields(partial) {
  const current = await getConfig();
  await invoke("save_config", { update: mergeUpdate(current, partial) });
}

export const getStatus = () => invoke("get_status");
export const getHistory = () => invoke("get_history");
export const clearHistory = () => invoke("clear_history");
