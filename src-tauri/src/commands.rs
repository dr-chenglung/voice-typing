//! 設定視窗用的 IPC commands（規格新增功能）。
//! API key 會明文回傳前端（2026-10 使用者要求）：設定頁預設以圓點遮住、按眼睛按鈕才顯示。
//! 原本只回報「是否已設定」、欄位永遠空白，使用者看不出已填哪一把而困惑；金鑰本來就以
//! 明文存在 %APPDATA% 的 config.toml，前端遮不遮對安全性影響不大。`save_config` 以欄位
//! 內容為準，留空代表清除金鑰。

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::config::Config;
use crate::history::{self, HistoryEntry};
use crate::transcribe;

#[derive(Serialize)]
pub struct ConfigView {
    pub stt_api_key: String,
    pub stt_api_url: String,
    pub stt_model: String,

    pub llm_api_key: String,
    pub llm_api_url: String,
    pub llm_model: String,

    pub enable_correction: bool,
    pub hotkey: String,

    pub vocabulary: String,
    pub enable_formatting: bool,
    /// 整理風格選「自訂」（文字另存於 custom_prompt.txt，見 `save_custom_prompt`）。
    pub use_custom_prompt: bool,

    pub target_language: String,
    pub translate_mode_active: bool,
}

#[derive(Deserialize)]
pub struct ConfigUpdate {
    /// 欄位內容即為要使用的 API key，留空＝清除。
    pub stt_api_key: String,
    pub stt_api_url: String,
    pub stt_model: String,

    /// 欄位內容即為要使用的 API key，留空＝清除。
    pub llm_api_key: String,
    pub llm_api_url: String,
    pub llm_model: String,

    pub enable_correction: bool,
    pub hotkey: String,

    pub vocabulary: String,
    pub enable_formatting: bool,
    /// 整理風格選「自訂」（文字另存於 custom_prompt.txt，見 `save_custom_prompt`）。
    pub use_custom_prompt: bool,

    pub target_language: String,
    pub translate_mode_active: bool,
}

#[tauri::command]
pub fn get_config(cfg: State<'_, Arc<Mutex<Config>>>) -> ConfigView {
    let c = cfg.lock().unwrap();
    ConfigView {
        stt_api_key: c.stt_api_key.clone(),
        stt_api_url: c.stt_api_url.clone(),
        stt_model: c.stt_model.clone(),
        llm_api_key: c.llm_api_key.clone(),
        llm_api_url: c.llm_api_url.clone(),
        llm_model: c.llm_model.clone(),
        enable_correction: c.enable_correction,
        hotkey: c.hotkey.clone(),
        vocabulary: c.vocabulary.clone(),
        enable_formatting: c.enable_formatting,
        use_custom_prompt: c.use_custom_prompt,
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
        // 選「自訂」但自訂文字還是空的：整個存檔拒絕、不改任何設定。
        validate_custom_prompt(update.use_custom_prompt, &c.custom_prompt)?;
        c.stt_api_key = update.stt_api_key.trim().to_string();
        c.stt_api_url = update.stt_api_url;
        c.stt_model = update.stt_model;
        c.llm_api_key = update.llm_api_key.trim().to_string();
        c.llm_api_url = update.llm_api_url;
        c.llm_model = update.llm_model;
        c.enable_correction = update.enable_correction;
        c.hotkey = update.hotkey;
        c.vocabulary = update.vocabulary;
        c.enable_formatting = update.enable_formatting;
        c.use_custom_prompt = update.use_custom_prompt;
        c.target_language = update.target_language;
        c.translate_mode_active = update.translate_mode_active;
        c.clone()
    };
    snapshot.save(&app).map_err(|e| e.to_string())
}

/// 選「自訂」時，自訂提示詞不可是空的（空白也算空），否則存檔會讓整理步驟送出空的 system prompt。
pub fn validate_custom_prompt(use_custom: bool, custom_prompt: &str) -> Result<(), String> {
    if use_custom && custom_prompt.trim().is_empty() {
        Err("自訂提示詞還是空的，請先寫內容，或按「從基本型複製」".to_string())
    } else {
        Ok(())
    }
}

/// 設定頁「AI 整理風格」要顯示的提示詞：內建兩種為實際送出的文字（不含個人詞彙表段），
/// 另附目前存的自訂提示詞。
#[derive(Serialize)]
pub struct StylePrompts {
    pub basic: String,
    pub active: String,
    pub custom: String,
}

#[tauri::command]
pub fn get_style_prompts(cfg: State<'_, Arc<Mutex<Config>>>) -> StylePrompts {
    let custom = cfg.lock().unwrap().custom_prompt.clone();
    StylePrompts {
        basic: transcribe::builtin_prompt(false),
        active: transcribe::builtin_prompt(true),
        custom,
    }
}

/// 存自訂提示詞（獨立檔案，不寫進 config.toml）。目前正選著「自訂」時不可存成空白。
#[tauri::command]
pub fn save_custom_prompt(
    app: AppHandle,
    cfg: State<'_, Arc<Mutex<Config>>>,
    text: String,
) -> Result<(), String> {
    let use_custom = cfg.lock().unwrap().use_custom_prompt;
    validate_custom_prompt(use_custom, &text)?;
    let dir = crate::config::config_dir(&app).map_err(|e| e.to_string())?;
    crate::config::write_custom_prompt(&dir, &text).map_err(|e| e.to_string())?;
    cfg.lock().unwrap().custom_prompt = text;
    Ok(())
}

#[tauri::command]
pub fn get_history(app: AppHandle) -> Vec<HistoryEntry> {
    history::load(&app)
}

#[tauri::command]
pub fn clear_history(app: AppHandle) -> Result<(), String> {
    history::clear(&app).map_err(|e| e.to_string())?;
    let _ = app.emit_to(crate::tray::MAIN_WINDOW, "history-cleared", ());
    Ok(())
}

/// 設定檔的完整路徑，一般設定頁顯示用。
#[tauri::command]
pub fn get_config_path(app: AppHandle) -> Result<String, String> {
    crate::config::config_path(&app)
        .map(|p| p.display().to_string())
        .map_err(|e| e.to_string())
}

/// 用檔案總管開啟設定檔所在的資料夾，方便備份。資料夾還不存在時先建立。
#[tauri::command]
pub fn open_config_folder(app: AppHandle) -> Result<(), String> {
    let path = crate::config::config_path(&app).map_err(|e| e.to_string())?;
    let dir = path.parent().ok_or("無法取得設定資料夾")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::process::Command::new("explorer")
        .arg(dir)
        .spawn()
        .map_err(|e| e.to_string())?;
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
            use_custom_prompt: false,
            target_language: target_language.to_string(),
            translate_mode_active,
        }
    }

    #[test]
    fn validate_custom_prompt_accepts_off_with_empty_text() {
        assert!(validate_custom_prompt(false, "").is_ok());
    }

    #[test]
    fn validate_custom_prompt_rejects_on_with_empty_text() {
        assert!(validate_custom_prompt(true, "").is_err());
    }

    #[test]
    fn validate_custom_prompt_rejects_on_with_whitespace_only() {
        assert!(validate_custom_prompt(true, "  \n\t ").is_err());
    }

    #[test]
    fn validate_custom_prompt_accepts_on_with_text() {
        assert!(validate_custom_prompt(true, "我的提示詞").is_ok());
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
