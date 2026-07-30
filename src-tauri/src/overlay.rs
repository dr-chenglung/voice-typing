//! 浮動麥克風圖示疊加視窗的生命週期管理（顯示/隱藏/定位/音量推播）。
//! 圖示本身改由前端 `ui/overlay/overlay.js`（canvas）繪製，這裡只管視窗。
//!
//! 視窗特性：
//!   - Tauri 內建：always_on_top、skip_taskbar、transparent、decorations(false)、click-through
//!   - 額外疊加 Win32 WS_EX_NOACTIVATE：Tauri 的 `.focused(false)` 只保證「建立時」不奪焦點，
//!     但本視窗會反覆 show()/hide() 重複使用，每次顯示仍可能被系統設為前景而偷走焦點，
//!     沒有對等的高層 API，所以仍需直接疊加這個 Win32 延伸樣式（規格 3.3 的核心要求）。

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

const WIN_W: f64 = 112.0; // 比底盤大一圈，留空間給音量發光效果的暈開範圍
const WIN_H: f64 = 112.0;
const BOTTOM_MARGIN: i32 = 36; // 距螢幕底部
const TICK: Duration = Duration::from_millis(33); // ~30fps，與舊版 FRAME 一致

/// 標記疊加視窗目前是否可見，供音量推播背景執行緒節流用。
struct OverlayVisible(Arc<AtomicBool>);

/// overlay-show 事件的 payload：標記是否進入翻譯模式。
#[derive(serde::Serialize, Clone)]
struct OverlayShowPayload {
    translate: bool,
}

/// 在 setup 階段建立隱藏的疊加視窗，並啟動音量推播背景執行緒。
/// `level` 由音訊執行緒寫入即時音量（f32 bits），與舊版相同。
pub fn create_window(app: &AppHandle, level: Arc<AtomicU32>) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(
        app,
        "overlay",
        WebviewUrl::App("overlay/index.html".into()),
    )
    .title("voice-overlay")
    .decorations(false)
    .resizable(false)
    .always_on_top(true)
    .visible(false)
    .focused(false)
    .skip_taskbar(true)
    .shadow(false)
    .transparent(true)
    .additional_browser_args(crate::WEBVIEW_ARGS)
    .inner_size(WIN_W, WIN_H)
    .build()?;

    apply_noactivate(&window);
    let _ = window.set_ignore_cursor_events(true);

    let visible = Arc::new(AtomicBool::new(false));
    app.manage(OverlayVisible(visible.clone()));

    let app_handle = app.clone();
    std::thread::spawn(move || loop {
        if visible.load(Ordering::Relaxed) {
            let raw = f32::from_bits(level.load(Ordering::Relaxed));
            let _ = app_handle.emit_to("overlay", "level", raw);
            std::thread::sleep(TICK);
        } else {
            std::thread::sleep(Duration::from_millis(200));
        }
    });

    Ok(())
}

/// 顯示疊加視窗：定位到螢幕底部置中、通知前端重置波形、以不奪焦點方式顯示。
/// `translate` 為 true 時前端會把麥克風圖示改成翻譯模式的配色（見 ui/overlay/overlay.js）。
pub fn show(app: &AppHandle, translate: bool) {
    let Some(window) = app.get_webview_window("overlay") else {
        return;
    };
    if let Ok(Some(monitor)) = window.current_monitor() {
        let ms = monitor.size();
        if let Ok(ws) = window.outer_size() {
            let x = (ms.width as i32 - ws.width as i32) / 2;
            let y = ms.height as i32 - ws.height as i32 - BOTTOM_MARGIN;
            let _ = window.set_position(PhysicalPosition::new(x, y));
        }
    }
    let _ = window.emit_to("overlay", "overlay-show", OverlayShowPayload { translate });
    let _ = window.show();
    if let Some(flag) = app.try_state::<OverlayVisible>() {
        flag.0.store(true, Ordering::Relaxed);
    }
}

/// 隱藏疊加視窗。
pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.hide();
    }
    if let Some(flag) = app.try_state::<OverlayVisible>() {
        flag.0.store(false, Ordering::Relaxed);
    }
}

/// 疊加 WS_EX_NOACTIVATE（+ WS_EX_TOOLWINDOW 雙重保險）：確保每次 show() 都不奪取輸入焦點。
fn apply_noactivate(window: &tauri::WebviewWindow) {
    if let Ok(raw) = window.hwnd() {
        let hwnd = raw.0 as isize as HWND;
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let add = (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW) as isize;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | add);
        }
    }
}
