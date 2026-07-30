//! 流程協調（規格第 6 節狀態機）。
//! 在獨立執行緒接收熱鍵 toggle 訊號，管理 cpal 錄音串流，並在停止後跑
//! STT → 校正 → enigo 的管線。網路請求用內建的 tokio runtime 以 block_on 執行；
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
use crate::state::AppState;
use crate::transcribe;
use crate::typer;
use crate::{overlay, tray};

pub fn run(
    rx: Receiver<()>,
    app: AppHandle,
    cfg: Arc<Mutex<Config>>,
    level: Arc<AtomicU32>,
) {
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

    // 每收到一個 toggle 就推進狀態機。
    while rx.recv().is_ok() {
        match state {
            AppState::Idle => match audio::start(level.clone()) {
                Ok(r) => {
                    recorder = Some(r);
                    state = AppState::Recording;
                    set_state(&app, state);
                    sound::play_start();
                }
                Err(e) => {
                    flash_error(&app, &format!("錄音啟動失敗: {e}"));
                    state = AppState::Idle;
                    set_state(&app, state);
                }
            },
            AppState::Recording => {
                sound::play_stop();
                state = AppState::Processing;
                set_state(&app, state);

                let rec = recorder.take().expect("Recording 狀態必有 recorder");
                let snapshot = cfg.lock().unwrap().clone();
                match process(&rt, &client, &snapshot, rec, &app) {
                    Ok(text) => {
                        // 不在此印「實際輸出」：常態下它就等於 process() 內印過的
                        // [corrected]（或校正關閉時的 [whisper]）；校正失敗屬例外，
                        // 已由 process() 走 stderr 的 [correct] 記錄，不混入例行輸出。
                        if let Err(e) = typer::type_text(&text) {
                            flash_error(&app, &format!("輸入失敗: {e}"));
                        } else {
                            // 目前一律傳「非翻譯」；翻譯模式的真實中繼資訊由 Task 8 的
                            // process()/Output 重構接上。
                            history::append(&app, &text, false, "", "");
                        }
                    }
                    Err(e) => flash_error(&app, &e.to_string()),
                }

                state = AppState::Idle;
                set_state(&app, state);
            }
            // Processing 期間忽略 toggle（規格：背景處理中不重複觸發）。
            AppState::Processing => {}
        }
    }
}

/// 依狀態更新托盤圖示，並同步 overlay 視窗顯示/隱藏（僅錄音中顯示）。
fn set_state(app: &AppHandle, state: AppState) {
    tray::set_state(app, state);
    match state {
        AppState::Recording => overlay::show(app),
        AppState::Processing | AppState::Idle => overlay::hide(app),
    }
}

/// 完整管線：停止錄音 → STT →（可選）校正。校正失敗降級為原始辨識文字。
fn process(
    rt: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    cfg: &Config,
    rec: Recorder,
    app: &AppHandle,
) -> Result<String> {
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
    // Whisper 原始辨識文字（未經 LLM 校正），印出供比對評估。
    println!("[whisper] {raw}");
    if !cfg.enable_correction {
        return Ok(raw);
    }
    // 校正失敗（含 LLM API key 未設定）→ 降級，不中止（規格 6.2）；
    // 降級不影響流程，但仍發系統通知讓使用者知道這次輸出是未校正的原始文字。
    let llm_key = match cfg.resolve_llm_api_key() {
        Ok(k) => k,
        Err(e) => {
            let msg = format!("{e}，已輸出原始辨識文字");
            eprintln!("[correct] {msg}");
            notify::show(app, "校正已略過", &msg);
            return Ok(raw);
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
            println!("[corrected] {c}"); // LLM 校正後文字，與上方 [whisper] 對照
            Ok(c)
        }
        Ok(_) => {
            // 校正無產出（空字串／回應無 content）：常見於本來就沒什麼內容的輸入，
            // 屬正常降級、非失敗，只記 log、不發通知（避免空輸入跳出多餘的錯誤訊息）。
            eprintln!("[correct] 校正無產出，已輸出原始辨識文字");
            Ok(raw)
        }
        Err(e) => {
            let msg = format!("校正失敗，已輸出原始辨識文字：{e}");
            eprintln!("[correct] {msg}");
            notify::show(app, "校正失敗", &msg);
            Ok(raw)
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
