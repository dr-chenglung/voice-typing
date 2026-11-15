//! 浮動疊加視窗的生命週期管理（顯示/隱藏/定位/波形與警告推播/失效自動重建）。
//! 膠囊本身（20 根波形條、秒數、音量警告、處理中進度條）由前端 `ui/overlay/` 以 DOM 繪製，這裡只管視窗與推播資料。
//!
//! 視窗特性：
//!   - Tauri 內建：always_on_top、skip_taskbar、transparent、decorations(false)、click-through
//!   - 不奪焦點（規格 3.3 的核心要求）：`.focused(false)` 只保證「建立時」不奪焦點，本視窗會反覆
//!     show()/hide()，所以另外設 `.focusable(false)`，讓 tao 自己帶上 WS_EX_NOACTIVATE。
//!     注意 tao 每次 show/hide 都會用它內部記錄的旗標「整組覆寫」延伸樣式，自行用
//!     SetWindowLongPtrW 疊加的樣式（見 `apply_noactivate`）會在第一次 show() 就被洗掉，
//!     只能在 tao 認得的旗標上設定才會持久。
//!
//! 顯示/隱藏一律「外層視窗＋webview」兩層一起切換（見 `show_both`／`hide`）：只切外層視窗的話，
//! WebView2 不會收到可見性變化，長時間隱藏後 Chromium 可能把頁面凍結。webview 層的 show() 會呼叫
//! `ICoreWebView2Controller::SetIsVisible(true)`，這是 WebView2 官方的喚醒途徑。
//!
//! 失效自動重建（看門狗）：上述喚醒仍不足以涵蓋所有情況——實測連續執行數小時後，仍會出現
//! 「有提示音、文字照常輸出，但膠囊不出現」，且只有重啟程式才恢復（推測是頁面凍結或
//! WebView2 renderer 行程失效，Tauri 不會自動復原）。因此每次顯示時都帶一個序號給前端，前端在
//! 下一個動畫影格回報 `overlay-alive`（證明 JS 與繪製迴圈都還在跑）；若 `ALIVE_TIMEOUT` 內沒收到
//! 回報，就判定這個 webview 已失效，建一個新視窗取代它（見 `recreate`）。
//! 新視窗用新 label（`overlay-1`、`overlay-2`…），先建好、確認活著才關掉舊的：若先關舊的，
//! 程式在那一瞬間會沒有任何視窗，Tauri 會視為「最後一個視窗關閉」而結束整個程式。

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{
    AppHandle, Emitter, Listener, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder,
};
use crate::audio::{LiveWarning, Meter};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

const WIN_W: f64 = 320.0; // 容納膠囊與上方的警告泡；固定大小，狀態切換時不需 resize／重新置中
const WIN_H: f64 = 100.0;
const BOTTOM_MARGIN: i32 = 32; // 距螢幕底部（視窗內膠囊下方另留 14px 透明邊給陰影，合計與舊版同高）
const TICK: Duration = Duration::from_millis(33); // ~30fps，與舊版 FRAME 一致
/// 警告判斷比波形慢：每 8 個 tick（約 264ms）才重算一次，其餘沿用上次結果。
const WARNING_EVERY_TICKS: u32 = 8;
/// 顯示後等前端回報 `overlay-alive` 的上限；正常情況下幾十毫秒內就會回報，
/// 放寬到 1.5 秒避免系統忙碌時誤判而無謂重建。
const ALIVE_TIMEOUT: Duration = Duration::from_millis(1500);
/// 重建時等新頁面載入完成並回報的上限。
const RECREATE_TIMEOUT: Duration = Duration::from_secs(5);

/// 目前應呈現的樣式，重建視窗後要據此還原畫面。
const MODE_HIDDEN: u8 = 0;
const MODE_RECORDING: u8 = 1;
const MODE_PROCESSING: u8 = 2;

/// 疊加視窗的共用狀態（以 `app.manage` 掛在 AppHandle 上）。
struct Overlay {
    /// 目前使用中的視窗 label；重建後會換成新的 label。
    label: Mutex<String>,
    /// 下一個重建視窗的編號，用來產生不重複的 label。
    generation: AtomicU32,
    /// 疊加視窗目前是否在推音量（錄音中），供音量推播背景執行緒節流用。
    visible: Arc<AtomicBool>,
    mode: AtomicU8,
    translate: AtomicBool,
    /// 本次錄音的起點，供失效重建時告訴前端「已經錄了多久」，秒數才不會歸零。
    started: Mutex<Option<Instant>>,
    /// 每次要求前端回報時遞增的序號，與前端回報的 `alive` 比對。
    show_seq: AtomicU64,
    /// 前端最近一次回報的序號。
    alive: Arc<AtomicU64>,
    /// 正在重建中，避免連續觸發多次重建。
    recreating: AtomicBool,
}

impl Overlay {
    fn label(&self) -> String {
        self.label.lock().unwrap().clone()
    }
}

/// overlay-show 事件的 payload：`translate` 標記是否進入翻譯模式，
/// `seq` 讓前端原樣回報 `overlay-alive`，供看門狗確認頁面還活著。
#[derive(serde::Serialize, Clone)]
struct OverlayShowPayload {
    translate: bool,
    seq: u64,
    /// 錄音已經過的毫秒數；一般顯示為 0，失效重建時為實際值。
    elapsed_ms: u64,
}

/// meter 事件的 payload：20 段原始 RMS（未正規化）與目前的音量警告。
#[derive(serde::Serialize, Clone)]
struct MeterPayload {
    bars: Vec<f32>,
    warning: LiveWarning,
}

/// 在 setup 階段建立隱藏的疊加視窗，並啟動音量推播背景執行緒。
/// `meter` 由音訊執行緒餵入樣本；這裡的背景執行緒每 tick 取 20 段波形與警告，以 `meter` 事件推給前端。
pub fn create_window(app: &AppHandle, meter: Arc<Meter>) -> tauri::Result<()> {
    build_window(app, "overlay")?;

    let visible = Arc::new(AtomicBool::new(false));
    let alive = Arc::new(AtomicU64::new(0));
    app.manage(Overlay {
        label: Mutex::new("overlay".to_string()),
        generation: AtomicU32::new(1),
        visible: visible.clone(),
        mode: AtomicU8::new(MODE_HIDDEN),
        translate: AtomicBool::new(false),
        started: Mutex::new(None),
        show_seq: AtomicU64::new(0),
        alive: alive.clone(),
        recreating: AtomicBool::new(false),
    });

    // 前端回報的序號只會遞增，用 fetch_max 避免較晚到的舊回報把值蓋小。
    app.listen_any("overlay-alive", move |event| {
        if let Ok(seq) = event.payload().parse::<u64>() {
            alive.fetch_max(seq, Ordering::Relaxed);
        }
    });

    let app_handle = app.clone();
    std::thread::spawn(move || {
        let mut tick: u32 = 0;
        let mut warning = LiveWarning::None;
        loop {
            if visible.load(Ordering::Relaxed) {
                // 先在這裡取值（Meter 內部的鎖用完即放），再去 emit，不在持鎖時呼叫 Tauri API。
                if tick % WARNING_EVERY_TICKS == 0 {
                    warning = meter.warning();
                }
                tick = tick.wrapping_add(1);
                let payload = MeterPayload {
                    bars: meter.bars(),
                    warning,
                };
                if let Some(state) = app_handle.try_state::<Overlay>() {
                    let _ = app_handle.emit_to(state.label().as_str(), "meter", payload);
                }
                std::thread::sleep(TICK);
            } else {
                tick = 0;
                warning = LiveWarning::None;
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    });

    Ok(())
}

/// 建立一個隱藏的疊加視窗（初次建立與失效重建共用）。
fn build_window(app: &AppHandle, label: &str) -> tauri::Result<tauri::WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::App("overlay/index.html".into()))
        .title("voice-overlay")
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .visible(false)
        .focused(false)
        .focusable(false)
        .skip_taskbar(true)
        .shadow(false)
        .transparent(true)
        .additional_browser_args(crate::WEBVIEW_ARGS)
        .inner_size(WIN_W, WIN_H)
        .build()?;

    apply_noactivate(&window);
    let _ = window.set_ignore_cursor_events(true);
    Ok(window)
}

/// 顯示疊加視窗：定位到螢幕底部置中、通知前端重置波形、以不奪焦點方式顯示。
/// `translate` 為 true 時前端會把膠囊改成翻譯模式的配色（藍紫，見 ui/overlay/overlay.js）。
/// 顯示後啟動看門狗：前端沒在時限內回報就重建視窗（理由見檔頭說明）。
pub fn show(app: &AppHandle, translate: bool) {
    let Some(state) = app.try_state::<Overlay>() else {
        return;
    };
    state.mode.store(MODE_RECORDING, Ordering::Relaxed);
    state.translate.store(translate, Ordering::Relaxed);
    *state.started.lock().unwrap() = Some(Instant::now());
    let Some(window) = app.get_webview_window(&state.label()) else {
        return;
    };
    place_at_bottom(&window);
    let seq = state.show_seq.fetch_add(1, Ordering::Relaxed) + 1;
    let _ = window.emit_to(
        window.label(),
        "overlay-show",
        OverlayShowPayload {
            translate,
            seq,
            elapsed_ms: 0,
        },
    );
    show_both(&window);
    state.visible.store(true, Ordering::Relaxed);

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(ALIVE_TIMEOUT);
        let Some(state) = app.try_state::<Overlay>() else {
            return;
        };
        if state.alive.load(Ordering::Relaxed) < seq {
            recreate(&app);
        }
    });
}

/// 切換為「處理中」樣式：錄音已結束、正在等 STT／LLM 回傳時呼叫。
/// 視窗維持顯示（錄音時本來就在畫面上），前端改畫不確定進度條＋「處理中…」，
/// 讓處理較久時使用者知道程式仍在運作。同時停止音量推播（此時已無音訊可推）。
pub fn processing(app: &AppHandle) {
    let Some(state) = app.try_state::<Overlay>() else {
        return;
    };
    state.visible.store(false, Ordering::Relaxed);
    state.mode.store(MODE_PROCESSING, Ordering::Relaxed);
    let Some(window) = app.get_webview_window(&state.label()) else {
        return;
    };
    let _ = window.emit_to(window.label(), "overlay-processing", ());
    show_both(&window);
}

/// 隱藏疊加視窗（webview 與外層視窗兩層都隱藏，理由見檔頭說明）。
pub fn hide(app: &AppHandle) {
    let Some(state) = app.try_state::<Overlay>() else {
        return;
    };
    state.visible.store(false, Ordering::Relaxed);
    state.mode.store(MODE_HIDDEN, Ordering::Relaxed);
    *state.started.lock().unwrap() = None;
    if let Some(window) = app.get_webview_window(&state.label()) {
        let webview: &tauri::Webview = window.as_ref();
        let _ = webview.hide();
        let _ = window.hide();
    }
}

/// 以新視窗取代已失效的疊加視窗，並依目前狀態（錄音中／處理中／隱藏）還原畫面。
/// 在看門狗背景執行緒呼叫，不在主執行緒上，所以可以同步等待新頁面載入。
fn recreate(app: &AppHandle) {
    let Some(state) = app.try_state::<Overlay>() else {
        return;
    };
    if state.recreating.swap(true, Ordering::Relaxed) {
        return;
    }
    let old_label = state.label();
    let new_label = format!(
        "overlay-{}",
        state.generation.fetch_add(1, Ordering::Relaxed)
    );
    eprintln!("[overlay] 視窗 {old_label} 沒有回應，重建為 {new_label}");

    let window = match build_window(app, &new_label) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[overlay] 重建疊加視窗失敗：{e}");
            state.recreating.store(false, Ordering::Relaxed);
            return;
        }
    };

    // 頁面載入完成、註冊好事件監聽之前送的事件會遺失，所以反覆送 overlay-show
    // 直到前端回報為止；回報同時也證明新頁面確實能正常執行。
    let deadline = Instant::now() + RECREATE_TIMEOUT;
    let ok = loop {
        let seq = state.show_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let translate = state.translate.load(Ordering::Relaxed);
        let elapsed_ms = state
            .started
            .lock()
            .unwrap()
            .map_or(0, |t| t.elapsed().as_millis() as u64);
        let _ = window.emit_to(
            new_label.as_str(),
            "overlay-show",
            OverlayShowPayload {
                translate,
                seq,
                elapsed_ms,
            },
        );
        std::thread::sleep(Duration::from_millis(100));
        if state.alive.load(Ordering::Relaxed) >= seq {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
    };
    if !ok {
        // 新視窗也沒回應：保留舊視窗不動，關掉新的，下次顯示時看門狗會再試一次。
        eprintln!("[overlay] 新視窗 {new_label} 也沒有回應，放棄本次重建");
        let _ = window.destroy();
        state.recreating.store(false, Ordering::Relaxed);
        return;
    }

    // 先切換 label（之後的 show/hide/音量推播都改用新視窗），再依目前狀態還原畫面。
    *state.label.lock().unwrap() = new_label;
    match state.mode.load(Ordering::Relaxed) {
        MODE_RECORDING => {
            place_at_bottom(&window);
            show_both(&window);
        }
        MODE_PROCESSING => {
            place_at_bottom(&window);
            let _ = window.emit_to(window.label(), "overlay-processing", ());
            show_both(&window);
        }
        _ => {}
    }
    if let Some(old) = app.get_webview_window(&old_label) {
        let _ = old.destroy();
    }
    state.recreating.store(false, Ordering::Relaxed);
}

/// 定位到目前螢幕底部置中。
fn place_at_bottom(window: &tauri::WebviewWindow) {
    if let Ok(Some(monitor)) = window.current_monitor() {
        let ms = monitor.size();
        if let Ok(ws) = window.outer_size() {
            let x = (ms.width as i32 - ws.width as i32) / 2;
            let y = ms.height as i32 - ws.height as i32 - BOTTOM_MARGIN;
            let _ = window.set_position(PhysicalPosition::new(x, y));
        }
    }
}

/// 先顯示外層視窗、再顯示 webview：後者會讓 WebView2 收到 SetIsVisible(true)，
/// 喚醒長時間隱藏後被凍結的頁面（理由見檔頭說明）。
fn show_both(window: &tauri::WebviewWindow) {
    let _ = window.show();
    let webview: &tauri::Webview = window.as_ref();
    let _ = webview.show();
}

/// 建立後立即疊加 WS_EX_NOACTIVATE（+ WS_EX_TOOLWINDOW）。只保證「第一次 show() 之前」有效，
/// 之後會被 tao 覆寫；持久的不奪焦點靠建立時的 `.focusable(false)`（見檔頭說明）。
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
