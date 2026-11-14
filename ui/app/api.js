// 呼叫 Rust 端的薄包裝。`window.__TAURI__` 由 tauri.conf.json 的 withGlobalTauri 注入，
// 放在函式裡才讀取，確保模組載入時不會因為它還沒就緒而出錯。

import { mergeUpdate } from "./lib/settings.js";

const tauri = () => window.__TAURI__;

export const invoke = (cmd, args) => tauri().core.invoke(cmd, args);
export const listen = (name, handler) => tauri().event.listen(name, handler);

/**
 * 註冊事件監聽，失敗只記錄、不往外丟。事件只負責「即時更新」，少了它頁面仍然可以使用；
 * 如果讓它丟出去，頁面載入流程會中斷，使用者只會看到一片空白。
 */
export async function listenSafely(name, handler) {
  try {
    await listen(name, handler);
  } catch (err) {
    console.error(`註冊事件 ${name} 失敗：`, err);
  }
}

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
