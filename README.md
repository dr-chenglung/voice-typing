# 語音免打字工具

按熱鍵錄音 → Groq Whisper 轉文字 → Groq LLM 輕度校正 → 模擬鍵盤打進當前焦點視窗。
另提供可選的**第二支翻譯熱鍵**，改用它錄音會把該次輸出翻譯成設定的目標語言，而非只做輕度校正。
常駐 Windows 系統列（工作列右下角通知區域）。完整需求見 [`voicetyping-spec.md`](./voicetyping-spec.md)。

## 環境需求

> 本專案以**原始碼形式**發布，未提供預建的安裝檔／執行檔，請自行建置（見下方 [建置與執行](#建置與執行)）。

- Windows 10/11（需 WebView2 Runtime；Win10/11 多數已預裝，缺少時可自行安裝 [Evergreen Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)）
- Rust（穩定版，MSVC toolchain）
- Tauri CLI：`cargo install tauri-cli`（一次性安裝；不需要 Node.js／npm）
- 一支 Groq API key（<https://console.groq.com>）

## 設定

API key 一律透過**設定視窗**填入：常駐系統列圖示按右鍵 → 「設定 Settings」，填入 API key 與其他選項後存檔，立即生效（熱鍵變更需重啟才生效）。不支援環境變數。

**權威存放位置是 `%APPDATA%\com.clhuang.voicetyping\config.toml`**，由設定視窗存檔時自動建立與寫入，不需要使用者自行準備範本檔；若該處沒有檔案，程式會向後相容讀取執行檔旁或目前工作目錄下的 `config.toml`。

STT（語音轉文字）與 LLM（校正）**各自獨立設定**，預設皆為 Groq，可改成其他 OpenAI 相容供應商：

| 欄位 | 說明 | 預設 |
|------|------|------|
| `stt_api_key` | STT 用的 API key | （空，需自行填入） |
| `stt_api_url` | STT 端點 URL | `https://api.groq.com/openai/v1/audio/transcriptions` |
| `stt_model` | 語音轉文字模型 | `whisper-large-v3-turbo` |
| `llm_api_key` | 校正用的 API key | （空，需自行填入） |
| `llm_api_url` | 校正端點 URL | `https://api.groq.com/openai/v1/chat/completions` |
| `llm_model` | 校正用模型 | `llama-3.1-8b-instant` |
| `enable_correction` | 是否做 LLM 校正 | `true` |
| `hotkey` | 觸發鍵，可改 `right_ctrl` 等 | `right_alt` |
| `target_language` | 翻譯輸出的目標語言（英文語言名稱），只在按翻譯熱鍵時生效 | `English` |
| `translate_hotkey` | 翻譯專用觸發鍵，留空＝停用；不可與 `hotkey` 相同 | `right_ctrl` |

## 建置與執行

```powershell
cargo install tauri-cli   # 一次性安裝
cd src-tauri
cargo tauri dev            # 開發模式
cargo tauri build          # 產出 release 安裝包（NSIS）與獨立 exe
```

## 使用方式

1. 啟動後常駐系統列（黑色麥克風圖示＝閒置）。
2. 把游標點進任一輸入框（記事本、瀏覽器…）。
3. 按 **右 Alt** → 螢幕底部中央浮現**麥克風圖示**，隨你說話的音量發光/微微放大 → 說一句話 → 再按 **右 Alt** 停止。
4. 圖示消失、系統列圖示變黃（處理中），數秒後校正完的文字自動打進游標處。
5. 系統列圖示按右鍵可開啟：
   - **設定 Settings**：調整 API key、模型、是否校正、熱鍵、翻譯目標語言與翻譯熱鍵（語音語言一律自動偵測，無需設定）。
   - **歷史紀錄 History**：瀏覽過去轉錄出的文字（持久化存於磁碟），翻譯過的項目會標示目標語言並可對照原文，可一鍵清除。
   - **結束 Quit**：離開程式。

> 底部麥克風圖示不奪取輸入焦點、可點擊穿透，不會影響你正在輸入的視窗。

### 翻譯輸出（第二支熱鍵）

除了主熱鍵，可另外設定一支**翻譯熱鍵**（預設 **右 Ctrl**）：用主熱鍵開始錄音會照常「錄音→校正」；改用翻譯熱鍵開始錄音，這次輸出會改成翻譯成「設定」視窗裡指定的**目標語言**（預設英文），錄音中途按哪一顆鍵停止都可以，不影響本次已決定要不要翻譯。底部麥克風圖示會用顏色區分目前是哪一種模式：**活力橙＝一般模式、藍紫＝翻譯模式**。翻譯熱鍵可在設定視窗停用（下拉選「不啟用」），也可依需要改成右 Shift／Scroll Lock／Pause／Insert 等其他鍵；翻譯熱鍵不可與主熱鍵設成同一顆鍵。

| 系統列圖示顏色 | 狀態 |
|----------|------|
| 黑 | 閒置 Idle |
| 紅 | 錄音中 Recording |
| 黃 | 處理中 Processing |
| 橘（短暫） | 發生錯誤（同時會跳出 Windows 系統通知說明原因） |

## 模組結構

後端（`src-tauri/src/`）：

| 檔案 | 職責 |
|------|------|
| `main.rs` | 進入點、執行緒接線、Tauri 事件迴圈 |
| `config.rs` | 設定檔讀取／儲存（`app_config_dir()` 為權威位置，向後相容舊位置） |
| `commands.rs` | 設定／歷史紀錄視窗用的 IPC commands |
| `hotkey.rs` | rdev 雙熱鍵監聽（主熱鍵＋翻譯熱鍵各自 toggle 偵測） |
| `audio.rs` | cpal 錄音 + 降取樣 + hound 封裝 WAV |
| `state.rs` | AppState 狀態定義；OutputMode（一般／翻譯） |
| `transcribe.rs` | Groq STT 與 LLM 校正／翻譯 |
| `controller.rs` | 狀態機協調、管線（一般校正／翻譯二選一）、容錯降級 |
| `typer.rs` | 剪貼簿貼上輸出（Ctrl+V，繞過中文輸入法組字；失敗時退回 enigo 打字） |
| `tray.rs` | 系統列圖示、狀態顯示、選單（設定／歷史紀錄／結束） |
| `overlay.rs` | 麥克風圖示疊加視窗的生命週期管理（顯示／隱藏／定位／NOACTIVATE／一般或翻譯配色） |
| `history.rs` | 轉錄文字歷史紀錄讀寫（`app_data_dir()/history.json`，含翻譯關聯欄位） |
| `notify.rs` | 背景執行緒失敗時發 Windows 系統通知 |

前端（純靜態 HTML/CSS/JS，無建置步驟，`ui/`）：

| 目錄 | 用途 |
|------|------|
| `ui/overlay/` | 麥克風圖示視窗：canvas 繪製黑色麥克風剪影，隨音量發光/縮放 |
| `ui/settings/` | 設定視窗表單 |
| `ui/history/` | 歷史紀錄清單與清除按鈕 |

## 已知限制（MVP）

- 僅 Windows。
- 校正失敗時會降級為輸出原始辨識文字（不中止）。
- 單次錄音上限約 12 分鐘（25MB），超過會報錯。
- 右 Alt 在部分歐語系鍵盤等同 AltGr；受影響者請改用 `hotkey = "right_ctrl"`。
- 輸出採剪貼簿貼上：輸出當下會短暫佔用剪貼簿（事後自動還原文字內容）；若原本剪貼簿是圖片等非文字內容，還原會略過。
- 熱鍵變更存檔後需重啟程式才會生效（`rdev::listen` 沒有乾淨的取消機制）；**翻譯熱鍵變更同樣需要重啟程式才生效**。
- **翻譯熱鍵不可與主熱鍵相同**：存檔時若兩者衝突會擋下存檔並提示；若既有 `config.toml` 剛好衝突（例如手改設定檔），啟動時會自動停用翻譯熱鍵並跳系統通知，不會導致程式崩潰。
- 歷史紀錄只保存**轉錄出的文字**（上限 500 筆，超過時 FIFO 淘汰最舊），錄音音訊本身仍是用完即丟、只存在記憶體，不落地。
