//! 系統托盤與三狀態回饋（規格第 3.3、6 節）。
//! 圖示用程式生成的麥克風造型（SS=4 超取樣反鋸齒），免外部圖檔；
//! 造型固定為「品牌藍圓角方塊底 + 白色麥克風剪影」，四種狀態外觀一致，
//! 狀態差異改由 tooltip 文字表達（見 `set_state`/`set_error`）。

use tauri::image::Image;
use tauri::menu::MenuBuilder;
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, Wry};

use crate::state::AppState;

const QUIT_ID: &str = "quit";
const SETTINGS_ID: &str = "settings";
const HISTORY_ID: &str = "history";

struct TrayHandle(TrayIcon<Wry>);

/// 建立托盤（預設 Idle 灰圖），存進 Tauri managed state 供 set_state/set_error 取用。
pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text(SETTINGS_ID, "設定 Settings")
        .text(HISTORY_ID, "歷史紀錄 History")
        .text(QUIT_ID, "結束 Quit")
        .build()?;

    let tray = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("語音免打字（閒置）")
        .icon(mic_icon())
        .on_menu_event(|app, event| {
            if event.id() == QUIT_ID {
                app.exit(0);
            } else if event.id() == SETTINGS_ID {
                open_settings_window(app);
            } else if event.id() == HISTORY_ID {
                open_history_window(app);
            }
        })
        .build(app)?;

    app.manage(TrayHandle(tray));
    Ok(())
}

/// 開啟設定視窗；若已開著則只是把它帶到前景，不會開出第二個。
fn open_settings_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(SETTINGS_ID) {
        let _ = w.set_focus();
        return;
    }
    // additional_browser_args 必須與其他視窗（尤其 overlay）一致，否則 WebView2 環境建立失敗、
    // 視窗開不出來（見 main.rs 的 WEBVIEW_ARGS 說明）。build 錯誤要印出來，不可靜默吞掉。
    if let Err(e) = WebviewWindowBuilder::new(
        app,
        SETTINGS_ID,
        WebviewUrl::App("settings/index.html".into()),
    )
    .title("設定 Settings")
    .inner_size(480.0, 780.0)
    .resizable(false)
    .additional_browser_args(crate::WEBVIEW_ARGS)
    .build()
    {
        eprintln!("[tray] 開啟設定視窗失敗: {e}");
    }
}

/// 開啟歷史紀錄視窗；若已開著則只是把它帶到前景，不會開出第二個。
fn open_history_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(HISTORY_ID) {
        let _ = w.set_focus();
        return;
    }
    // 同 open_settings_window：參數必須與其他視窗一致，build 錯誤要印出來。
    if let Err(e) = WebviewWindowBuilder::new(
        app,
        HISTORY_ID,
        WebviewUrl::App("history/index.html".into()),
    )
    .title("歷史紀錄 History")
    .inner_size(480.0, 600.0)
    .additional_browser_args(crate::WEBVIEW_ARGS)
    .build()
    {
        eprintln!("[tray] 開啟歷史紀錄視窗失敗: {e}");
    }
}

/// 更新閒置狀態的 tooltip，附加目前的翻譯模式；由呼叫端（main.rs 啟動時、
/// controller.rs 狀態機回到 Idle 時）保證只在真的處於 Idle 時呼叫。
pub fn set_idle_tooltip(app: &AppHandle, translate_mode_active: bool, target_language: &str) {
    let mode_suffix = if translate_mode_active {
        format!("｜翻譯模式 → {target_language}")
    } else {
        "｜轉錄模式".to_string()
    };
    let tip = format!("語音免打字（閒置{mode_suffix}）");
    if let Some(tray) = app.try_state::<TrayHandle>() {
        let _ = tray.0.set_icon(Some(mic_icon()));
        let _ = tray.0.set_tooltip(Some(tip));
    }
}

/// 依狀態切換 tooltip；圖示外觀四種狀態一致（白底黑色麥克風），不再隨狀態變色。
pub fn set_state(app: &AppHandle, state: AppState) {
    let tip = match state {
        AppState::Idle => "語音免打字（閒置）",
        AppState::Recording => "🔴 錄音中…再按熱鍵停止",
        AppState::Processing => "⏳ 處理中…",
    };
    if let Some(tray) = app.try_state::<TrayHandle>() {
        let _ = tray.0.set_icon(Some(mic_icon()));
        let _ = tray.0.set_tooltip(Some(tip));
    }
}

/// 短暫顯示錯誤狀態（圖示不變，僅 tooltip 顯示訊息）。
pub fn set_error(app: &AppHandle, msg: &str) {
    if let Some(tray) = app.try_state::<TrayHandle>() {
        let _ = tray.0.set_icon(Some(mic_icon()));
        let _ = tray.0.set_tooltip(Some(format!("⚠ {msg}")));
    }
}

/// 麥克風剪影固定白色，背景固定品牌藍圓角方塊：彩色方塊與工作列的黑/白/灰階背景
/// 天生有色相差異，不論淺色或深色工作列主題都能維持對比，不會被同色系背景吃掉。
const MIC_FG_RGB: (u8, u8, u8) = (255, 255, 255);
const BG_RGB: (u8, u8, u8) = (59, 130, 246);

/// 產生 32x32 圖示：品牌藍圓角方塊底 + 白色麥克風剪影疊在上面，方塊外完全透明。
/// 設計座標系固定在 32x32（與最終輸出尺寸一致），用 SS=4 超取樣後平均降採樣做反鋸齒，
/// 剪影與底塊分別取覆蓋率再用 over 合成，讓兩者交界也有反鋸齒。
fn mic_icon() -> Image<'static> {
    const SIZE: u32 = 32;
    const SS: u32 = 4;
    const HI: u32 = SIZE * SS;

    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    for oy in 0..SIZE {
        for ox in 0..SIZE {
            let mut mic_cov = 0u32;
            let mut bg_cov = 0u32;
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = (ox * SS + sx) as f32 + 0.5;
                    let y = (oy * SS + sy) as f32 + 0.5;
                    if mic_shape_covers(x, y, HI as f32) {
                        mic_cov += 1;
                    }
                    if bg_rounded_square_covers(x, y, HI as f32) {
                        bg_cov += 1;
                    }
                }
            }
            let total = (SS * SS) as f32;
            let mic_a = mic_cov as f32 / total;
            let bg_a = bg_cov as f32 / total;
            // over 合成：黑色剪影疊在白色底塊上，底塊疊在透明背景上。
            let out_a = mic_a + bg_a * (1.0 - mic_a);
            let (out_r, out_g, out_b) = if out_a > 0.0 {
                let mix = |fg: u8, bg: u8| -> u8 {
                    let v = (fg as f32 * mic_a + bg as f32 * bg_a * (1.0 - mic_a)) / out_a;
                    (v + 0.5) as u8
                };
                (
                    mix(MIC_FG_RGB.0, BG_RGB.0),
                    mix(MIC_FG_RGB.1, BG_RGB.1),
                    mix(MIC_FG_RGB.2, BG_RGB.2),
                )
            } else {
                (0, 0, 0)
            };
            let idx = ((oy * SIZE + ox) * 4) as usize;
            rgba[idx] = out_r;
            rgba[idx + 1] = out_g;
            rgba[idx + 2] = out_b;
            rgba[idx + 3] = (out_a * 255.0 + 0.5) as u8;
        }
    }
    Image::new_owned(rgba, SIZE, SIZE)
}

/// 背景圓角方塊範圍：置中方塊，四邊留一點邊距（工作列縮放時不貼邊），角落做圓角。
/// 邊距比先前版本略縮小，讓方塊整體再放大一點。
fn bg_rounded_square_covers(x: f32, y: f32, canvas: f32) -> bool {
    let s = canvas / 32.0;
    let margin = 1.0 * s;
    let corner_r = 7.0 * s;
    let half = 16.0 * s - margin;
    let cx = canvas / 2.0;
    let dx = (x - cx).abs();
    let dy = (y - cx).abs();
    if dx > half || dy > half {
        return false;
    }
    let inner = half - corner_r;
    if dx <= inner || dy <= inner {
        true
    } else {
        let ex = dx - inner;
        let ey = dy - inner;
        ex * ex + ey * ey <= corner_r * corner_r
    }
}

/// 標準麥克風剪影（與 overlay 的 mic-fill SVG 同造型）：圓角機身 + U 形支架 + 直柱 + 底座。
/// 設計基準座標系為 32x32，`canvas` 是實際取樣畫布邊長（含超取樣倍數），
/// 用比例縮放讓形狀不受取樣解析度影響。
fn mic_shape_covers(x: f32, y: f32, canvas: f32) -> bool {
    let s = canvas / 32.0;
    let cx = canvas / 2.0;

    // 機身（膠囊）：中軸 (cx, 6s)~(cx, 14s)，半徑 4.5s。
    let body_r = 4.5 * s;
    let body_top = 6.0 * s;
    let body_bot = 14.0 * s;
    let in_body = if y < body_top {
        dist(x, y, cx, body_top) <= body_r
    } else if y > body_bot {
        dist(x, y, cx, body_bot) <= body_r
    } else {
        (x - cx).abs() <= body_r
    };

    // 支架（U 形托）：以機身底部圓心 (cx, 14s) 為圓心的厚弧，只取下半圈，像話筒的腳架。
    let arc_center = body_bot;
    let arc_outer = 9.0 * s;
    let arc_thick = 1.7 * s;
    let d = dist(x, y, cx, arc_center);
    let in_bracket = y >= arc_center && d <= arc_outer && d >= arc_outer - arc_thick;

    // 直柱：U 形托底部往下到底座的細柱。
    let stem_top = arc_center + arc_outer - arc_thick;
    let stem_bot = 28.0 * s;
    let in_stem = (x - cx).abs() <= 0.9 * s && y >= stem_top && y <= stem_bot;

    // 底座：水平短橫。
    let in_base = (x - cx).abs() <= 4.5 * s && y >= stem_bot && y <= stem_bot + 1.6 * s;

    in_body || in_bracket || in_stem || in_base
}

fn dist(x: f32, y: f32, cx: f32, cy: f32) -> f32 {
    ((x - cx) * (x - cx) + (y - cy) * (y - cy)).sqrt()
}
