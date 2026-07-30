//! 設定檔讀取/儲存（規格第 7 節）。
//! 從 config.toml 讀取；API key 一律透過設定視窗填入，不支援環境變數、不硬編碼。
//! 權威存放位置是 `app_config_dir()`（Windows: `%APPDATA%\<identifier>\config.toml`），
//! 因為安裝後執行檔所在目錄通常沒有寫入權限；讀取時向後相容舊版（執行檔旁/當前目錄）的設定檔。

use anyhow::{bail, Context, Result};
use rdev::Key;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub stt_api_key: String,
    #[serde(default = "default_stt_api_url")]
    pub stt_api_url: String,
    #[serde(default = "default_stt")]
    pub stt_model: String,

    #[serde(default)]
    pub llm_api_key: String,
    #[serde(default = "default_llm_api_url")]
    pub llm_api_url: String,
    #[serde(default = "default_llm")]
    pub llm_model: String,

    #[serde(default = "default_true")]
    pub enable_correction: bool,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,

    /// 個人詞彙表：常用專有名詞，逗號或換行分隔。空字串＝不啟用。
    /// 同時用於 STT（Whisper `prompt` 參數，引導拼字）與 LLM 校正提示（修正音譯/誤辨）。
    #[serde(default)]
    pub vocabulary: String,
    /// 智慧排版（opt-in，預設關閉）：開啟時長內容允許分段/條列並輕度濃縮成要點；
    /// 關閉時維持「只做輕度清理、不條列」的原規則。
    #[serde(default)]
    pub enable_formatting: bool,
    /// 翻譯輸出的目標語言（英文語言名稱，如 "English"／"Japanese"／"Traditional Chinese"）。
    /// 只決定「翻成什麼」，是否翻譯完全由按下哪顆熱鍵決定，與此欄位無關。
    #[serde(default = "default_target_language")]
    pub target_language: String,
    /// 翻譯專用熱鍵；空字串＝停用翻譯功能。不可與 `hotkey` 相同（見 `resolved_translate_key`）。
    #[serde(default = "default_translate_hotkey")]
    pub translate_hotkey: String,
}

fn default_stt_api_url() -> String {
    "https://api.groq.com/openai/v1/audio/transcriptions".to_string()
}
fn default_llm_api_url() -> String {
    "https://api.groq.com/openai/v1/chat/completions".to_string()
}
fn default_stt() -> String {
    "whisper-large-v3".to_string()
}
fn default_llm() -> String {
    "openai/gpt-oss-120b".to_string()
}
fn default_true() -> bool {
    true
}
fn default_hotkey() -> String {
    "right_alt".to_string()
}
fn default_target_language() -> String {
    "English".to_string()
}
fn default_translate_hotkey() -> String {
    "right_ctrl".to_string()
}

impl Config {
    /// 依序找 app_config_dir/config.toml（權威位置）→ 執行檔目錄 → 當前目錄；
    /// 找不到則全用預設值。
    pub fn load(app: &AppHandle) -> Result<Self> {
        let cfg = match find_config(app) {
            Some(path) => {
                let s = std::fs::read_to_string(&path)
                    .with_context(|| format!("讀取設定檔失敗: {}", path.display()))?;
                println!("[config] 載入設定檔: {}", path.display());
                toml::from_str(&s).with_context(|| "設定檔格式錯誤（config.toml）")?
            }
            None => {
                println!("[config] 找不到 config.toml，使用預設值（API key 需在設定視窗填入）");
                // 空字串解析即得到全部 serde 預設值。
                toml::from_str("").unwrap()
            }
        };
        Ok(cfg)
    }

    /// 取得 STT API key：讀 config.toml 的 stt_api_key。
    pub fn resolve_stt_api_key(&self) -> Result<String> {
        resolve_key(&self.stt_api_key, "STT")
    }

    /// 取得 LLM 校正 API key：讀 config.toml 的 llm_api_key。
    pub fn resolve_llm_api_key(&self) -> Result<String> {
        resolve_key(&self.llm_api_key, "LLM 校正")
    }

    /// 寫入 app_config_dir/config.toml（設定視窗存檔用，從此成為權威位置）。
    pub fn save(&self, app: &AppHandle) -> Result<()> {
        let path = config_path(app)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("建立設定目錄失敗: {}", parent.display()))?;
        }
        let s = toml::to_string_pretty(self).context("設定序列化失敗")?;
        std::fs::write(&path, s).with_context(|| format!("寫入設定檔失敗: {}", path.display()))?;
        println!("[config] 已儲存設定檔: {}", path.display());
        Ok(())
    }

    /// 解析翻譯熱鍵設定：
    /// - 欄位空字串 → `Ok(None)`（使用者明確不啟用，非錯誤）。
    /// - 能解析且與主熱鍵不同的按鍵 → `Ok(Some(key))`（啟用）。
    /// - 無法辨識，或解析後與主熱鍵是同一顆鍵 → `Err(訊息)`，呼叫端應停用翻譯熱鍵並提示使用者。
    pub fn resolved_translate_key(&self) -> Result<Option<Key>, String> {
        let raw = self.translate_hotkey.trim();
        if raw.is_empty() {
            return Ok(None);
        }
        let Some(key) = crate::hotkey::parse_key(raw) else {
            return Err(format!("無法辨識的翻譯熱鍵設定: {raw:?}"));
        };
        match crate::hotkey::parse_key(&self.hotkey) {
            Some(main_key) if main_key == key => Err(format!(
                "翻譯熱鍵與主熱鍵相同（{raw:?}），已停用翻譯功能"
            )),
            _ => Ok(Some(key)),
        }
    }
}

fn resolve_key(from_cfg: &str, label: &str) -> Result<String> {
    let from_cfg = from_cfg.trim();
    if !from_cfg.is_empty() {
        return Ok(from_cfg.to_string());
    }
    bail!("找不到{label} API key：請在設定視窗填入");
}

fn config_path(app: &AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_config_dir()
        .context("無法取得設定目錄（app_config_dir）")?;
    Ok(dir.join("config.toml"))
}

fn find_config(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(dir) = app.path().app_config_dir() {
        let p = dir.join("config.toml");
        if p.exists() {
            return Some(p);
        }
    }
    // 向後相容：設定視窗導入前，使用者習慣手改的位置。
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("config.toml");
            if p.exists() {
                return Some(p);
            }
        }
    }
    let cwd = PathBuf::from("config.toml");
    if cwd.exists() {
        return Some(cwd);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(hotkey: &str, translate_hotkey: &str) -> Config {
        let mut cfg: Config = toml::from_str("").unwrap();
        cfg.hotkey = hotkey.to_string();
        cfg.translate_hotkey = translate_hotkey.to_string();
        cfg
    }

    #[test]
    fn defaults_are_english_and_right_ctrl() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.target_language, "English");
        assert_eq!(cfg.translate_hotkey, "right_ctrl");
    }

    #[test]
    fn resolved_translate_key_enabled_by_default() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(matches!(cfg.resolved_translate_key(), Ok(Some(_))));
    }

    #[test]
    fn resolved_translate_key_none_when_empty() {
        let cfg = make_config("right_alt", "");
        assert!(matches!(cfg.resolved_translate_key(), Ok(None)));
    }

    #[test]
    fn resolved_translate_key_conflict_when_same_as_main() {
        let cfg = make_config("right_alt", "right_alt");
        assert!(cfg.resolved_translate_key().is_err());
    }

    #[test]
    fn resolved_translate_key_conflict_across_aliases() {
        // "alt_right" 與 "right_alt" 解析後是同一顆鍵（Key::AltGr），也要視為衝突。
        let cfg = make_config("right_alt", "alt_right");
        assert!(cfg.resolved_translate_key().is_err());
    }

    #[test]
    fn resolved_translate_key_err_when_unparseable() {
        let cfg = make_config("right_alt", "no_such_key");
        assert!(cfg.resolved_translate_key().is_err());
    }
}
