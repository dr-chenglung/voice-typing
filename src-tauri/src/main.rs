// release 版不開額外的命令列視窗（雙擊時不會閃黑框）；debug 版保留 console 方便看 log。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! 語音免打字工具 — Tauri 進入點與執行緒接線（規格第 6.1 執行緒模型）。
//!
//! 執行緒分工：
//!   - 主執行緒：Tauri/webview 事件迴圈（tray 圖示回饋、overlay 視窗顯示）。
//!   - 熱鍵執行緒：rdev::listen 偵測右 Alt（可設定）。
//!   - controller 執行緒：管理 cpal 錄音、跑 STT/校正/enigo 管線（內含 tokio runtime）。
//! 三者以 channel 與 AppHandle 溝通，背景工作絕不阻塞主執行緒與熱鍵執行緒。

mod audio;
mod commands;
mod config;
mod controller;
mod history;
mod hotkey;
mod notify;
mod overlay;
mod sound;
mod state;
mod transcribe;
mod tray;
mod typer;

use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};

use tauri::Manager;

/// 全 app 共用的 WebView2/Chromium 啟動參數，**所有視窗都必須套用同一組**。
///
/// 為什麼要共用：wry 會替每個 webview 各自建立一個 WebView2 環境
/// （`CreateCoreWebView2EnvironmentWithOptions`），但它們共用同一個 user data 資料夾；
/// WebView2 的限制是「共用資料夾的環境，啟動參數必須一致」——瀏覽器行程用第一個建立的
/// 環境參數啟動，之後任何要求不同參數的環境都會建立失敗（ERROR_INVALID_STATE）。
/// 若只有 overlay 帶自訂參數、settings/history 用預設參數，開設定/歷史視窗就會失敗（視窗開不出來）。
///
/// 內容：前段是 wry 的預設值（一旦自訂 additional_browser_args 就會整串覆蓋，必須手動保留）；
/// 後三個開關關掉「隱藏/被遮蔽視窗的背景節流」，讓 overlay 被 hide() 後前端的
/// requestAnimationFrame 繪製迴圈不被暫停，下次 show() 才畫得出麥克風（對一般視窗無害）。
pub(crate) const WEBVIEW_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
    --disable-background-timer-throttling \
    --disable-backgrounding-occluded-windows \
    --disable-renderer-backgrounding";

fn main() {
    // 麥克風即時音量（f32 bits），由音訊執行緒寫入、overlay 視窗讀取。
    let level = Arc::new(AtomicU32::new(0));
    // 熱鍵執行緒 → controller 執行緒 的 toggle 訊號。
    let (tx, rx) = std::sync::mpsc::channel::<()>();

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::get_history,
            commands::clear_history,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            // 設定檔讀取需要 app_config_dir()，只能在 setup 階段（已有 AppHandle）才做。
            let cfg = config::Config::load(&handle).expect("載入設定失敗");
            let hotkey = hotkey::parse_key(&cfg.hotkey).unwrap_or_else(|| {
                // 降級而非 panic：舊版設定視窗曾短暫提供過的熱鍵選項（如 scroll_lock）
                // 若殘留在 config.toml，不能讓 release 版（隱藏 console）直接啟動失敗、
                // 使用者卻連設定視窗都打不開去修正。
                let msg = format!(
                    "無法辨識的熱鍵設定 {:?}，已回復為預設的右 Alt，請到設定視窗重新選擇",
                    cfg.hotkey
                );
                eprintln!("[config] {msg}");
                notify::show(&handle, "熱鍵設定已重設", &msg);
                rdev::Key::AltGr
            });

            println!("================ 語音免打字工具 ================");
            println!("熱鍵: {}（toggle：按一下開始錄音、再按一下停止）", cfg.hotkey);
            println!(
                "翻譯模式: {}",
                if cfg.translate_mode_active {
                    format!("開啟 → {}", cfg.target_language)
                } else {
                    "關閉".to_string()
                }
            );
            println!(
                "STT: {} | 校正: {}（{}）",
                cfg.stt_model,
                cfg.llm_model,
                if cfg.enable_correction { "開啟" } else { "關閉" }
            );
            println!("常駐系統托盤；於托盤選單按「設定」可調整、按「結束」可離開。");
            println!("===============================================");

            let initial_translate_mode = cfg.translate_mode_active;
            let initial_target_language = cfg.target_language.clone();
            let cfg_shared = Arc::new(Mutex::new(cfg));
            app.manage(cfg_shared.clone());

            // 托盤與疊加視窗必須在主執行緒建立（setup hook 保證在主執行緒執行）。
            tray::build(&handle)?;
            tray::set_idle_tooltip(&handle, initial_translate_mode, &initial_target_language);
            overlay::create_window(&handle, level.clone())?;

            // 熱鍵監聽執行緒。
            std::thread::spawn(move || hotkey::run(hotkey, tx));

            // controller 執行緒（管理錄音 + 管線）。
            let ctrl_handle = handle.clone();
            let ctrl_level = level.clone();
            std::thread::spawn(move || controller::run(rx, ctrl_handle, cfg_shared, ctrl_level));

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri 應用程式啟動失敗");
}
