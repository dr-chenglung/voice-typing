//! 設定檔讀取/儲存（規格第 7 節）。
//! 從 config.toml 讀取；API key 一律透過設定視窗填入，不支援環境變數、不硬編碼。
//! 權威存放位置是 `app_config_dir()`（Windows: `%APPDATA%\<identifier>\config.toml`），
//! 因為安裝後執行檔所在目錄通常沒有寫入權限；讀取時向後相容舊版（執行檔旁/當前目錄）的設定檔。

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// 自訂整理提示詞的檔名，放在 `app_config_dir()`。獨立於 config.toml：舊版 0.2.x 存檔時
/// 會洗掉它不認得的欄位，文字放在另一個檔案才不會因為切回舊版而遺失。
const CUSTOM_PROMPT_FILE: &str = "custom_prompt.txt";

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
    /// 只決定「翻成什麼」，是否翻譯由 `translate_mode_active` 決定，與此欄位無關。
    #[serde(default = "default_target_language")]
    pub target_language: String,
    /// 翻譯模式開關：開啟時，下一次用熱鍵開始的錄音會被翻譯成 target_language，
    /// 而非一般校正。開始錄音當下讀取，不需要重啟程式即可生效。
    #[serde(default)]
    pub translate_mode_active: bool,
    /// 整理風格選「自訂」：用 `custom_prompt` 取代內建提示詞。只存這個開關，文字不放 config.toml。
    #[serde(default)]
    pub use_custom_prompt: bool,
    /// 自訂提示詞全文，啟動時從 `custom_prompt.txt` 載入；`#[serde(skip)]` 讓它不會寫進 config.toml。
    #[serde(skip)]
    pub custom_prompt: String,
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

impl Config {
    /// 依序找 app_config_dir/config.toml（權威位置）→ 執行檔目錄 → 當前目錄；
    /// 找不到則全用預設值。
    pub fn load(app: &AppHandle) -> Result<Self> {
        let mut cfg: Config = match find_config(app) {
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
        if let Ok(dir) = config_dir(app) {
            cfg.custom_prompt = read_custom_prompt(&dir);
        }
        Ok(cfg)
    }

    /// 執行時真正要用的自訂提示詞：必須選了「自訂」**而且**文字非空白才回 `Some`。
    /// 選了自訂但文字不見了（檔案被刪、被清空）時回 `None`，由呼叫端退回內建風格，
    /// 避免把空的 system prompt 送給模型。
    pub fn active_custom_prompt(&self) -> Option<&str> {
        if self.use_custom_prompt && !self.custom_prompt.trim().is_empty() {
            Some(self.custom_prompt.as_str())
        } else {
            None
        }
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
}

fn resolve_key(from_cfg: &str, label: &str) -> Result<String> {
    let from_cfg = from_cfg.trim();
    if !from_cfg.is_empty() {
        return Ok(from_cfg.to_string());
    }
    bail!("找不到{label} API key：請在設定視窗填入");
}

/// 設定資料夾（Windows：`%APPDATA%\com.clhuang.voicetyping`），config.toml 與自訂提示詞都放這裡。
pub fn config_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .app_config_dir()
        .context("無法取得設定目錄（app_config_dir）")
}

pub fn config_path(app: &AppHandle) -> Result<PathBuf> {
    Ok(config_dir(app)?.join("config.toml"))
}

/// 讀自訂提示詞；檔案不存在或讀不到都當作沒有（回空字串）。
pub fn read_custom_prompt(dir: &Path) -> String {
    std::fs::read_to_string(dir.join(CUSTOM_PROMPT_FILE)).unwrap_or_default()
}

/// 寫自訂提示詞；資料夾還不存在時先建立。
pub fn write_custom_prompt(dir: &Path, text: &str) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("建立設定目錄失敗: {}", dir.display()))?;
    let path = dir.join(CUSTOM_PROMPT_FILE);
    std::fs::write(&path, text).with_context(|| format!("寫入自訂提示詞失敗: {}", path.display()))
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

    #[test]
    fn target_language_default_is_english() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.target_language, "English");
    }

    #[test]
    fn translate_mode_active_defaults_to_false() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(!cfg.translate_mode_active);
    }

    #[test]
    fn translate_mode_active_round_trips_through_toml() {
        let mut cfg: Config = toml::from_str("").unwrap();
        cfg.translate_mode_active = true;
        let s = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert!(back.translate_mode_active);
    }

    fn unique_temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("voicetyping-test-{tag}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn use_custom_prompt_defaults_to_false() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(!cfg.use_custom_prompt);
    }

    /// 自訂提示詞的文字只存在獨立檔案，不進 config.toml：舊版 0.2.x 存檔時會洗掉它不認得的欄位。
    #[test]
    fn custom_prompt_text_is_never_written_into_config_toml() {
        let mut cfg: Config = toml::from_str("").unwrap();
        cfg.use_custom_prompt = true;
        cfg.custom_prompt = "秘密提示詞".to_string();
        let s = toml::to_string_pretty(&cfg).unwrap();
        assert!(!s.contains("秘密提示詞"));
        assert!(s.contains("use_custom_prompt = true"));
    }

    #[test]
    fn custom_prompt_file_round_trips_and_missing_file_reads_as_empty() {
        let dir = unique_temp_dir("prompt");
        assert_eq!(read_custom_prompt(&dir), "");
        write_custom_prompt(&dir, "第一行\n第二行").unwrap();
        assert_eq!(read_custom_prompt(&dir), "第一行\n第二行");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 選了自訂但文字不見了或是空的，執行時必須退回內建風格，不能把空的 system prompt 送出去。
    #[test]
    fn active_custom_prompt_requires_flag_and_text() {
        let mut cfg: Config = toml::from_str("").unwrap();
        for (flag, text, expected) in [
            (false, "x", None),
            (true, "", None),
            (true, "  \n", None),
            (true, "x", Some("x")),
        ] {
            cfg.use_custom_prompt = flag;
            cfg.custom_prompt = text.to_string();
            assert_eq!(cfg.active_custom_prompt(), expected, "flag={flag} text={text:?}");
        }
    }
}
