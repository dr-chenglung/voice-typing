//! 熱鍵監聽（規格第 3.1 節）。
//! 在獨立執行緒以 rdev::listen 監聽鍵盤，同時偵測主熱鍵（預設右 Alt）與可選的翻譯熱鍵的
//! 「按下」transition，各自按下送出對應的 `OutputMode`。每顆鍵各自有獨立的 down 旗標，
//! 濾掉按住自動重複的 KeyPress（見 `run()`）。
//!
//! 可用鍵（見 `parse_key`）：right_alt／right_ctrl／scroll_lock／pause／insert，共 5 種。
//! 注意：right_shift 刻意不提供——持續右 Shift 打大寫字母是常見操作，會與正常打字衝突。

use crate::state::OutputMode;
use rdev::{listen, Event, EventType, Key};
use std::sync::mpsc::Sender;

/// 把設定字串轉成 rdev::Key。Windows 上右 Alt 回報為 AltGr、右 Ctrl 回報為 ControlRight。
pub fn parse_key(s: &str) -> Option<Key> {
    let norm = s.to_lowercase().replace([' ', '-'], "_");
    match norm.as_str() {
        "right_alt" | "alt_right" | "altgr" | "ralt" => Some(Key::AltGr),
        "right_ctrl" | "ctrl_right" | "right_control" | "rctrl" => Some(Key::ControlRight),
        "scroll_lock" | "scrolllock" => Some(Key::ScrollLock),
        "pause" | "pause_break" => Some(Key::Pause),
        "insert" | "ins" => Some(Key::Insert),
        _ => None,
    }
}

/// 阻塞式監聽迴圈，應在獨立執行緒呼叫。同時監聽主熱鍵與（可選的）翻譯熱鍵：
/// 主熱鍵按下送 `OutputMode::Direct`，翻譯熱鍵按下送 `OutputMode::Translate`。
/// `translate_key` 為 `None` 時只監聽主熱鍵，行為與翻譯功能停用前完全相同。
pub fn run(main_key: Key, translate_key: Option<Key>, tx: Sender<OutputMode>) {
    let mut main_down = false;
    let mut translate_down = false;
    let callback = move |event: Event| match event.event_type {
        EventType::KeyPress(k) if k == main_key => {
            if !main_down {
                main_down = true;
                let _ = tx.send(OutputMode::Direct);
            }
        }
        EventType::KeyRelease(k) if k == main_key => {
            main_down = false;
        }
        EventType::KeyPress(k) if translate_key == Some(k) => {
            if !translate_down {
                translate_down = true;
                let _ = tx.send(OutputMode::Translate);
            }
        }
        EventType::KeyRelease(k) if translate_key == Some(k) => {
            translate_down = false;
        }
        _ => {}
    };
    if let Err(e) = listen(callback) {
        eprintln!("[hotkey] rdev listen 失敗: {:?}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_right_alt_aliases() {
        assert_eq!(parse_key("right_alt"), Some(Key::AltGr));
        assert_eq!(parse_key("Right-Alt"), Some(Key::AltGr));
        assert_eq!(parse_key("altgr"), Some(Key::AltGr));
    }

    #[test]
    fn parses_right_ctrl_aliases() {
        assert_eq!(parse_key("right_ctrl"), Some(Key::ControlRight));
    }

    #[test]
    fn parses_scroll_lock() {
        assert_eq!(parse_key("scroll_lock"), Some(Key::ScrollLock));
        assert_eq!(parse_key("scrolllock"), Some(Key::ScrollLock));
    }

    #[test]
    fn parses_pause() {
        assert_eq!(parse_key("pause"), Some(Key::Pause));
    }

    #[test]
    fn parses_insert() {
        assert_eq!(parse_key("insert"), Some(Key::Insert));
        assert_eq!(parse_key("ins"), Some(Key::Insert));
    }

    #[test]
    fn rejects_unknown_key() {
        assert_eq!(parse_key("caps_lock"), None);
    }
}
