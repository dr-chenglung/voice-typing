//! 歷史紀錄讀寫（規格新增功能）。
//! 落地持久化的是「轉錄後文字」，跟 CLAUDE.md 規定的「錄音音訊用完即丟、只在記憶體」
//! 是不同條規則——那條只管原始音訊，這裡存的是文字，使用者已確認要保留並可清除。

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const MAX_HISTORY_ENTRIES: usize = 500;
const FILE_NAME: &str = "history.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Unix epoch 秒數；前端自行依使用者時區格式化顯示。
    pub timestamp: u64,
    pub text: String,
    /// 此筆是否為翻譯輸出（而非一般校正/原始輸出）。
    #[serde(default)]
    pub translated: bool,
    /// 翻譯目標語言；`translated` 為 false 時為空字串。
    #[serde(default)]
    pub target_language: String,
    /// 翻譯前的 STT 原文，供歷史紀錄視窗對照；`translated` 為 false 時為空字串。
    #[serde(default)]
    pub source_text: String,
}

/// 成功輸出一筆文字後呼叫；新筆插入最前面（新到舊排序），超過上限淘汰最舊的。
/// `translated`／`target_language`／`source_text`：一般模式輸出傳 `false, "", ""`。
pub fn append(
    app: &AppHandle,
    text: &str,
    translated: bool,
    target_language: &str,
    source_text: &str,
) {
    let mut entries = load(app);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    entries.insert(
        0,
        HistoryEntry {
            timestamp,
            text: text.to_string(),
            translated,
            target_language: target_language.to_string(),
            source_text: source_text.to_string(),
        },
    );
    entries.truncate(MAX_HISTORY_ENTRIES);
    if let Err(e) = write(app, &entries) {
        eprintln!("[history] 寫入歷史紀錄失敗: {e}");
    }
}

/// 讀取全部歷史（新到舊）；檔案不存在或格式錯誤時回傳空清單。
pub fn load(app: &AppHandle) -> Vec<HistoryEntry> {
    let Ok(path) = history_path(app) else {
        return Vec::new();
    };
    let Ok(s) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&s).unwrap_or_default()
}

/// 清空歷史紀錄（落地刪除，重啟後仍是空的）。
pub fn clear(app: &AppHandle) -> Result<()> {
    write(app, &[])
}

fn write(app: &AppHandle, entries: &[HistoryEntry]) -> Result<()> {
    let path = history_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let s = serde_json::to_string_pretty(entries)?;
    std::fs::write(&path, s)?;
    Ok(())
}

fn history_path(app: &AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow!("無法取得資料目錄（app_data_dir）: {e}"))?;
    Ok(dir.join(FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_old_entry_missing_new_fields() {
        let json = r#"{"timestamp": 100, "text": "hello"}"#;
        let entry: HistoryEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.timestamp, 100);
        assert_eq!(entry.text, "hello");
        assert!(!entry.translated);
        assert_eq!(entry.target_language, "");
        assert_eq!(entry.source_text, "");
    }

    #[test]
    fn round_trips_translated_entry() {
        let entry = HistoryEntry {
            timestamp: 1,
            text: "Hello".to_string(),
            translated: true,
            target_language: "English".to_string(),
            source_text: "你好".to_string(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: HistoryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.target_language, "English");
        assert_eq!(back.source_text, "你好");
    }
}
