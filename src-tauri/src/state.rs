//! 應用程式狀態定義。

use serde::Serialize;

/// 規格第 3.1 節：以此狀態變數控制 toggle 流程。
/// 加 Serialize 是因為要透過 `app_handle.emit()` 把狀態變化送到前端（設定/歷史視窗可能顯示）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppState {
    Idle,
    Recording,
    Processing,
}

/// 錄音本次的輸出模式：由開始錄音當下讀取的 `translate_mode_active` 設定值決定，全程不變（規格 3.1）。
/// 不衍生 Serialize：僅用於後端 channel 傳遞與比較，不會直接送到前端。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputMode {
    Direct,
    Translate,
}
