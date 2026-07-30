//! 設定視窗用的 IPC commands（規格新增功能）。
//! API key 不可明文回傳前端：`get_config` 只回報「是否已設定」，`save_config` 的
//! `api_key` 留空代表「不變更」，避免明文塞進 DOM／devtools 可見。

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::config::Config;
use crate::history::{self, HistoryEntry};

#[derive(Serialize)]
pub struct ConfigView {
    pub has_stt_api_key: bool,
    pub stt_api_url: String,
    pub stt_model: String,

    pub has_llm_api_key: bool,
    pub llm_api_url: String,
    pub llm_model: String,

    pub enable_correction: bool,
    pub hotkey: String,

    pub vocabulary: String,
    pub enable_formatting: bool,

    pub target_language: String,
    pub translate_mode_active: bool,
}

#[derive(Deserialize)]
pub struct ConfigUpdate {
    /// 留空字串＝不變更現有 API key。
    pub stt_api_key: String,
    pub stt_api_url: String,
    pub stt_model: String,

    /// 留空字串＝不變更現有 API key。
    pub llm_api_key: String,
    pub llm_api_url: String,
    pub llm_model: String,

    pub enable_correction: bool,
    pub hotkey: String,

    pub vocabulary: String,
    pub enable_formatting: bool,

    pub target_language: String,
    pub translate_mode_active: bool,
}

#[tauri::command]
pub fn get_config(cfg: State<'_, Arc<Mutex<Config>>>) -> ConfigView {
    let c = cfg.lock().unwrap();
    ConfigView {
        has_stt_api_key: !c.stt_api_key.trim().is_empty(),
        stt_api_url: c.stt_api_url.clone(),
        stt_model: c.stt_model.clone(),
        has_llm_api_key: !c.llm_api_key.trim().is_empty(),
        llm_api_url: c.llm_api_url.clone(),
        llm_model: c.llm_model.clone(),
        enable_correction: c.enable_correction,
        hotkey: c.hotkey.clone(),
        vocabulary: c.vocabulary.clone(),
        enable_formatting: c.enable_formatting,
        target_language: c.target_language.clone(),
        translate_mode_active: c.translate_mode_active,
    }
}

/// 驗證主熱鍵設定：必須能被 `hotkey::parse_key` 解析，否則下次啟動會 panic（見 `main.rs`）。
fn validate_main_hotkey(hotkey: &str) -> Result<(), String> {
    match crate::hotkey::parse_key(hotkey.trim()) {
        Some(_) => Ok(()),
        None => Err(format!("無法辨識的熱鍵設定: {hotkey:?}")),
    }
}

/// 驗證目標語言：不可留空，否則翻譯 system prompt 會組出「翻譯成【】」這種壞掉的提示詞
/// （見 `transcribe::build_translate_system_prompt` 的防呆 fallback，這裡是第一道防線）。
fn validate_target_language(target_language: &str) -> Result<(), String> {
    if target_language.trim().is_empty() {
        Err("目標語言不可留空".to_string())
    } else {
        Ok(())
    }
}

/// 存檔前的全部驗證，依序執行、任何一項失敗就整體拒絕存檔（不修改任何設定狀態）。
fn validate_config_update(update: &ConfigUpdate) -> Result<(), String> {
    validate_main_hotkey(&update.hotkey)?;
    validate_target_language(&update.target_language)?;
    Ok(())
}

#[tauri::command]
pub fn save_config(
    app: AppHandle,
    cfg: State<'_, Arc<Mutex<Config>>>,
    update: ConfigUpdate,
) -> Result<(), String> {
    validate_config_update(&update)?;
    let snapshot = {
        let mut c = cfg.lock().unwrap();
        if !update.stt_api_key.trim().is_empty() {
            c.stt_api_key = update.stt_api_key.trim().to_string();
        }
        c.stt_api_url = update.stt_api_url;
        c.stt_model = update.stt_model;
        if !update.llm_api_key.trim().is_empty() {
            c.llm_api_key = update.llm_api_key.trim().to_string();
        }
        c.llm_api_url = update.llm_api_url;
        c.llm_model = update.llm_model;
        c.enable_correction = update.enable_correction;
        c.hotkey = update.hotkey;
        c.vocabulary = update.vocabulary;
        c.enable_formatting = update.enable_formatting;
        c.target_language = update.target_language;
        c.translate_mode_active = update.translate_mode_active;
        c.clone()
    };
    snapshot.save(&app).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_history(app: AppHandle) -> Vec<HistoryEntry> {
    history::load(&app)
}

#[tauri::command]
pub fn clear_history(app: AppHandle) -> Result<(), String> {
    history::clear(&app).map_err(|e| e.to_string())?;
    let _ = app.emit_to("history", "history-cleared", ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_main_hotkey_accepts_known_key() {
        assert!(validate_main_hotkey("right_ctrl").is_ok());
    }

    #[test]
    fn validate_main_hotkey_rejects_unparseable() {
        assert!(validate_main_hotkey("caps_lock").is_err());
    }

    #[test]
    fn validate_target_language_rejects_empty() {
        assert!(validate_target_language("").is_err());
    }

    #[test]
    fn validate_target_language_rejects_whitespace_only() {
        assert!(validate_target_language("   ").is_err());
    }

    #[test]
    fn validate_target_language_accepts_non_empty() {
        assert!(validate_target_language("English").is_ok());
    }

    fn sample_update(hotkey: &str, target_language: &str, translate_mode_active: bool) -> ConfigUpdate {
        ConfigUpdate {
            stt_api_key: String::new(),
            stt_api_url: String::new(),
            stt_model: String::new(),
            llm_api_key: String::new(),
            llm_api_url: String::new(),
            llm_model: String::new(),
            enable_correction: true,
            hotkey: hotkey.to_string(),
            vocabulary: String::new(),
            enable_formatting: false,
            target_language: target_language.to_string(),
            translate_mode_active,
        }
    }

    #[test]
    fn validate_config_update_rejects_unparseable_main_hotkey() {
        let update = sample_update("caps_lock", "English", false);
        assert!(validate_config_update(&update).is_err());
    }

    #[test]
    fn validate_config_update_rejects_empty_target_language() {
        let update = sample_update("right_alt", "   ", false);
        assert!(validate_config_update(&update).is_err());
    }

    #[test]
    fn validate_config_update_accepts_valid_update() {
        let update = sample_update("right_alt", "English", true);
        assert!(validate_config_update(&update).is_ok());
    }
}
