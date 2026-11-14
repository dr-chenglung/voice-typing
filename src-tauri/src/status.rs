//! 目前狀態（閒置／錄音中／處理中）的儲存與推送，供主視窗的狀態列與主頁使用。

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::AppState;
use crate::tray::MAIN_WINDOW;

/// 目前的 App 狀態。controller 執行緒每次切換狀態時寫入，主視窗開啟時用 `get_status` 讀取，
/// 這樣錄音中才打開主視窗，狀態列一開就是正確的，不必等下一次狀態變化。
pub struct CurrentStatus(Mutex<AppState>);

impl CurrentStatus {
    pub fn new() -> Self {
        Self(Mutex::new(AppState::Idle))
    }

    pub fn get(&self) -> AppState {
        *self.0.lock().unwrap()
    }

    pub fn set(&self, state: AppState) {
        *self.0.lock().unwrap() = state;
    }
}

/// `app-status` 事件與 `get_status` 指令共用的內容。
#[derive(Serialize, Clone)]
pub struct StatusPayload {
    pub state: AppState,
}

/// 記下新狀態，並通知主視窗。主視窗沒開時 emit 會失敗，直接忽略。
/// 這裡不碰 `Config` 的鎖，所以可以從 `controller::set_state` 安全呼叫。
pub fn publish(app: &AppHandle, state: AppState) {
    if let Some(current) = app.try_state::<CurrentStatus>() {
        current.set(state);
    }
    let _ = app.emit_to(MAIN_WINDOW, "app-status", StatusPayload { state });
}

#[tauri::command]
pub fn get_status(status: State<'_, CurrentStatus>) -> StatusPayload {
    StatusPayload { state: status.get() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_status_starts_idle() {
        assert_eq!(CurrentStatus::new().get(), AppState::Idle);
    }

    #[test]
    fn current_status_returns_last_set_value() {
        let s = CurrentStatus::new();
        s.set(AppState::Recording);
        s.set(AppState::Processing);
        assert_eq!(s.get(), AppState::Processing);
    }

    #[test]
    fn status_payload_serializes_snake_case() {
        let json = serde_json::to_string(&StatusPayload { state: AppState::Recording }).unwrap();
        assert_eq!(json, r#"{"state":"recording"}"#);
    }
}
