//! 流程協調（規格第 6 節狀態機）。
//! 在獨立執行緒接收熱鍵 toggle 訊號（帶輸出模式），管理 cpal 錄音串流，並在停止後跑
//! STT → （校正或翻譯）→ enigo 的管線。網路請求用內建的 tokio runtime 以 block_on 執行；
//! 本執行緒即「背景工作執行緒」，不會阻塞熱鍵監聽或主執行緒（托盤/overlay）。

use std::sync::atomic::AtomicU32;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Result};
use tauri::AppHandle;

use crate::audio::{self, Recorder};
use crate::config::Config;
use crate::history;
use crate::notify;
use crate::sound;
use crate::state::{AppState, OutputMode};
use crate::transcribe;
use crate::typer;
use crate::{overlay, tray};

/// `process()` 的輸出：最終要打字輸出的文字，以及供歷史紀錄使用的翻譯中繼資訊。
struct Output {
    text: String,
    translated: bool,
    target_language: String,
    source_text: String,
}

impl Output {
    /// 非翻譯輸出（一般模式，或翻譯降級為原文時）的簡便建構式。
    fn direct(text: String) -> Self {
        Self {
            text,
            translated: false,
            target_language: String::new(),
            source_text: String::new(),
        }
    }
}

pub fn run(rx: Receiver<OutputMode>, app: AppHandle, cfg: Arc<Mutex<Config>>, level: Arc<AtomicU32>) {
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[controller] 建立 tokio runtime 失敗: {e}");
            return;
        }
    };
    let client = reqwest::Client::new();

    let mut state = AppState::Idle;
    let mut recorder: Option<Recorder> = None;
    // 本次錄音的輸出模式：由「開始錄音當下收到的訊號」決定，全程不因中途按錯鍵而改變。
    let mut session_mode = OutputMode::Direct;

    // 每收到一個訊號就推進狀態機。
    while let Ok(mode) = rx.recv() {
        match state {
            AppState::Idle => match audio::start(level.clone()) {
                Ok(r) => {
                    recorder = Some(r);
                    session_mode = mode;
                    state = AppState::Recording;
                    set_state(&app, state, session_mode);
                    sound::play_start();
                }
                Err(e) => {
                    flash_error(&app, &format!("錄音啟動失敗: {e}"));
                    state = AppState::Idle;
                    set_state(&app, state, session_mode);
                }
            },
            AppState::Recording => {
                sound::play_stop();
                state = AppState::Processing;
                set_state(&app, state, session_mode);

                let rec = recorder.take().expect("Recording 狀態必有 recorder");
                let snapshot = cfg.lock().unwrap().clone();
                match process(&rt, &client, &snapshot, rec, session_mode, &app) {
                    Ok(out) => {
                        if let Err(e) = typer::type_text(&out.text) {
                            flash_error(&app, &format!("輸入失敗: {e}"));
                        } else {
                            history::append(
                                &app,
                                &out.text,
                                out.translated,
                                &out.target_language,
                                &out.source_text,
                            );
                        }
                    }
                    Err(e) => flash_error(&app, &e.to_string()),
                }

                state = AppState::Idle;
                set_state(&app, state, session_mode);
            }
            // Processing 期間忽略訊號（規格：背景處理中不重複觸發）。
            AppState::Processing => {}
        }
    }
}

/// 依狀態更新托盤圖示，並同步 overlay 視窗顯示/隱藏（僅錄音中顯示）；
/// `mode` 決定 overlay 顯示一般或翻譯配色。
fn set_state(app: &AppHandle, state: AppState, mode: OutputMode) {
    tray::set_state(app, state);
    match state {
        AppState::Recording => overlay::show(app, mode == OutputMode::Translate),
        AppState::Processing | AppState::Idle => overlay::hide(app),
    }
}

/// 完整管線：停止錄音 → STT → 依模式分派到一般校正或翻譯。
fn process(
    rt: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    cfg: &Config,
    rec: Recorder,
    mode: OutputMode,
    app: &AppHandle,
) -> Result<Output> {
    let stt_key = cfg.resolve_stt_api_key()?;
    let wav = rec.stop_to_wav()?;
    let raw = rt.block_on(transcribe::transcribe(
        client,
        &stt_key,
        &cfg.stt_api_url,
        &cfg.stt_model,
        &cfg.vocabulary,
        wav,
    ))?;
    if raw.is_empty() {
        bail!("STT（Whisper）辨識結果為空：本次錄音可能沒有收到聲音、時間太短，或內容無法辨識，請確認麥克風音量後再試一次");
    }
    // Whisper 原始辨識文字（未經校正/翻譯），印出供比對評估。
    println!("[whisper] {raw}");

    match mode {
        OutputMode::Direct => process_direct(rt, client, cfg, raw, app),
        OutputMode::Translate => process_translate(rt, client, cfg, raw, app),
    }
}

/// 一般模式：可選校正，失敗（含 LLM key 未設定）降級為原始辨識文字（規格 6.2）。
fn process_direct(
    rt: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    cfg: &Config,
    raw: String,
    app: &AppHandle,
) -> Result<Output> {
    if !cfg.enable_correction {
        return Ok(Output::direct(raw));
    }
    let llm_key = match cfg.resolve_llm_api_key() {
        Ok(k) => k,
        Err(e) => {
            let msg = format!("{e}，已輸出原始辨識文字");
            eprintln!("[correct] {msg}");
            notify::show(app, "校正已略過", &msg);
            return Ok(Output::direct(raw));
        }
    };
    match rt.block_on(transcribe::correct(
        client,
        &llm_key,
        &cfg.llm_api_url,
        &cfg.llm_model,
        &raw,
        &cfg.vocabulary,
        cfg.enable_formatting,
    )) {
        Ok(c) if !c.is_empty() => {
            println!("[corrected] {c}"); // 校正後文字，與上方 [whisper] 對照
            Ok(Output::direct(c))
        }
        Ok(_) => {
            // 校正無產出：常見於本來就沒什麼內容的輸入，屬正常降級、非失敗，只記 log、不發通知。
            eprintln!("[correct] 校正無產出，已輸出原始辨識文字");
            Ok(Output::direct(raw))
        }
        Err(e) => {
            let msg = format!("校正失敗，已輸出原始辨識文字：{e}");
            eprintln!("[correct] {msg}");
            notify::show(app, "校正失敗", &msg);
            Ok(Output::direct(raw))
        }
    }
}

/// 翻譯模式：不受 `enable_correction` 影響，降級規則比照校正（LLM key 未設定/呼叫失敗→通知，
/// 回應空字串→只記 log）。
fn process_translate(
    rt: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    cfg: &Config,
    raw: String,
    app: &AppHandle,
) -> Result<Output> {
    let llm_key = match cfg.resolve_llm_api_key() {
        Ok(k) => k,
        Err(e) => {
            let msg = format!("{e}，已輸出原始辨識文字");
            eprintln!("[translate] {msg}");
            notify::show(app, "翻譯已略過", &msg);
            return Ok(Output::direct(raw));
        }
    };
    match rt.block_on(transcribe::translate(
        client,
        &llm_key,
        &cfg.llm_api_url,
        &cfg.llm_model,
        &raw,
        &cfg.vocabulary,
        &cfg.target_language,
        cfg.enable_formatting,
    )) {
        Ok(t) if !t.is_empty() => {
            println!("[translated] {t}"); // 翻譯後文字，與上方 [whisper] 對照
            Ok(Output {
                text: t,
                translated: true,
                target_language: cfg.target_language.clone(),
                source_text: raw,
            })
        }
        Ok(_) => {
            eprintln!("[translate] 翻譯無產出，已輸出原始辨識文字");
            Ok(Output::direct(raw))
        }
        Err(e) => {
            let msg = format!("翻譯失敗，已輸出原始辨識文字：{e}");
            eprintln!("[translate] {msg}");
            notify::show(app, "翻譯失敗", &msg);
            Ok(Output::direct(raw))
        }
    }
}

/// 托盤短暫顯示錯誤（停留約 1.2 秒）並發系統通知，兩者都給，避免使用者沒看到 tooltip 就錯過。
fn flash_error(app: &AppHandle, msg: &str) {
    eprintln!("[error] {msg}");
    tray::set_error(app, msg);
    notify::show(app, "語音免打字發生錯誤", msg);
    overlay::hide(app);
    std::thread::sleep(Duration::from_millis(1200));
}
