//! 錄音與 WAV 封裝（規格第 4 節）。
//! cpal 抓麥克風到記憶體 buffer（混為單聲道），停止時降取樣到 16kHz 並用 hound 封裝成 WAV bytes，
//! 全程不落地。Recorder 持有 cpal::Stream（!Send），僅在 controller 執行緒內使用。

use anyhow::{anyhow, bail, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::io::Cursor;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

const TARGET_RATE: u32 = 16_000;
const MAX_WAV_BYTES: usize = 25 * 1024 * 1024; // Groq 25MB 上限
/// 靜音判定的音量門檻（RMS，樣本為 f32 [-1,1]）。0.003 約等於 -50 dBFS。
///
/// 這個值是 2026-08-16 兩次實機回報夾出來的，不是理論值：
///   - 初版 0.01（-40 dBFS，教科書上的「說話 vs 底噪」分界）→ 使用者正常說話被誤判為沒聲音，
///     要「超級大聲」才過得了關 ⇒ 該裝置的說話音量 **低於 0.01**。
///   - 改為 0.001（-60 dBFS）→ 完全沒出聲的錄音仍被送去辨識並生出文字
///     ⇒ 該裝置的閒置底噪 **高於 0.001**。
/// 兩者夾出 (0.001, 0.01)，取幾何中間值 0.003。單元測試 `is_silent_returns_true_for_idle_microphone_hiss`
/// （0.002 算沒說話）與 `is_silent_returns_false_for_speech_on_a_low_gain_microphone`
/// （0.005 算有說話）就是這個門檻的上下界，改動時務必同時確認。
///
/// 注意這台裝置的底噪與說話音量相當接近，可調空間不大；要再調整**務必對照
/// `stop_to_wav` 印出的 `[audio] 本次錄音最大視窗 RMS` 實測值**，不要憑估計。
const SILENCE_RMS_THRESHOLD: f32 = 0.003;
/// 靜音判定的視窗長度（毫秒）。夠短才抓得到很簡短的一句話。
const SILENCE_WINDOW_MS: usize = 30;

pub struct Recorder {
    stream: cpal::Stream,
    buf: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
}

/// 開始錄音：開啟預設輸入裝置的串流，PCM 持續寫入記憶體 buffer。
/// `level` 由本函式的音訊 callback 持續更新為當前音量（f32 bits），供波形疊加視窗讀取。
pub fn start(level: Arc<AtomicU32>) -> Result<Recorder> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("找不到麥克風輸入裝置"))?;
    let supported = device
        .default_input_config()
        .map_err(|e| anyhow!("取得輸入設定失敗: {e}"))?;
    let sample_rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    let buf = Arc::new(Mutex::new(Vec::<f32>::new()));
    let buf_cb = buf.clone();
    let level_cb = level.clone();
    let err_fn = |e| eprintln!("[audio] 串流錯誤: {e}");

    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _: &_| push_mono(&buf_cb, data, channels, &level_cb),
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _: &_| {
                let f: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                push_mono(&buf_cb, &f, channels, &level_cb);
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config,
            move |data: &[u16], _: &_| {
                let f: Vec<f32> = data.iter().map(|&s| (s as f32 - 32768.0) / 32768.0).collect();
                push_mono(&buf_cb, &f, channels, &level_cb);
            },
            err_fn,
            None,
        ),
        other => bail!("不支援的取樣格式: {other:?}"),
    }
    .map_err(|e| anyhow!("建立輸入串流失敗: {e}"))?;

    stream.play().map_err(|e| anyhow!("啟動串流失敗: {e}"))?;
    println!("[audio] 開始錄音（{sample_rate} Hz, {channels} ch → 16kHz mono）");
    Ok(Recorder {
        stream,
        buf,
        sample_rate,
    })
}

impl Recorder {
    /// 停止錄音並回傳 16kHz 單聲道 16-bit WAV bytes（記憶體內）。
    ///
    /// 整段都沒偵測到說話時回 `Ok(None)`（見 `is_silent`），呼叫端據此直接跳過 STT，
    /// 避免把純靜音送去 Whisper 而拿到幻覺文字（例如「謝謝觀看」）。
    /// 注意「完全沒收到樣本」仍是錯誤而非 `None`：那代表裝置真的沒送資料，是該讓使用者知道的問題。
    pub fn stop_to_wav(self) -> Result<Option<Vec<u8>>> {
        let Recorder {
            stream,
            buf,
            sample_rate,
        } = self;
        drop(stream); // 停止擷取

        let samples = std::mem::take(&mut *buf.lock().unwrap());
        if samples.is_empty() {
            bail!("沒有錄到任何音訊");
        }
        // 實測值一律印出（不論判定結果），供日後調整 SILENCE_RMS_THRESHOLD 時對照。
        println!(
            "[audio] 本次錄音最大視窗 RMS {:.5}（靜音門檻 {SILENCE_RMS_THRESHOLD}）",
            max_window_rms(&samples, sample_rate)
        );
        if is_silent(&samples, sample_rate) {
            println!("[audio] 整段音量皆低於門檻，判定為未說話，跳過辨識");
            return Ok(None);
        }
        let resampled = resample_linear(&samples, sample_rate, TARGET_RATE);
        let wav = encode_wav_16k(&resampled)?;
        if wav.len() > MAX_WAV_BYTES {
            bail!(
                "錄音超過 25MB 上限（{} bytes），請縮短單次錄音時間",
                wav.len()
            );
        }
        println!("[audio] 停止錄音，WAV {} bytes", wav.len());
        Ok(Some(wav))
    }
}

/// 把交錯的多聲道資料混成單聲道後追加到 buffer，並更新當前音量（RMS，含衰減平滑）。
fn push_mono(buf: &Arc<Mutex<Vec<f32>>>, data: &[f32], channels: usize, level: &AtomicU32) {
    let mut b = buf.lock().unwrap();
    let start = b.len();
    if channels <= 1 {
        b.extend_from_slice(data);
    } else {
        for frame in data.chunks(channels) {
            let sum: f32 = frame.iter().sum();
            b.push(sum / channels as f32);
        }
    }
    // 以本批新樣本算 RMS，再與前值取衰減最大值，讓波形不會瞬間歸零。
    let new = &b[start..];
    if !new.is_empty() {
        let sum_sq: f32 = new.iter().map(|s| s * s).sum();
        let rms = (sum_sq / new.len() as f32).sqrt();
        let prev = f32::from_bits(level.load(Ordering::Relaxed));
        let smoothed = rms.max(prev * 0.80);
        level.store(smoothed.to_bits(), Ordering::Relaxed);
    }
}

/// 判斷整段錄音是否從頭到尾都沒有人說話。
///
/// 做法是把樣本切成 `SILENCE_WINDOW_MS` 毫秒的視窗逐一算 RMS（均方根音量），
/// 只要**任何一個**視窗超過 `SILENCE_RMS_THRESHOLD` 就視為有說話。刻意不用整段平均：
/// 長錄音裡只講了短短一句時，平均值會被大量靜音稀釋而誤判成沒說話，那種「該送卻沒送」
/// 比放過一句 Whisper 幻覺文字嚴重得多。
/// 樣本數不足一個視窗時，整段當成一個視窗處理。
pub fn is_silent(samples: &[f32], sample_rate: u32) -> bool {
    max_window_rms(samples, sample_rate) <= SILENCE_RMS_THRESHOLD
}

/// 整段錄音中「最大聲的那個視窗」的 RMS，也就是 `is_silent` 拿來跟門檻比較的值。
/// 另外印進 log 供調整 `SILENCE_RMS_THRESHOLD` 時對照真實錄音的數字。
/// 沒有樣本時回 0.0。
pub fn max_window_rms(samples: &[f32], sample_rate: u32) -> f32 {
    let window = ((sample_rate as usize * SILENCE_WINDOW_MS) / 1000).max(1);
    samples
        .chunks(window)
        .map(|w| {
            let sum_sq: f32 = w.iter().map(|s| s * s).sum();
            (sum_sq / w.len() as f32).sqrt()
        })
        .fold(0.0f32, f32::max)
}

/// 線性內插降/升取樣。MVP 用簡單內插即可（規格未要求高品質重採樣）。
fn resample_linear(input: &[f32], in_rate: u32, out_rate: u32) -> Vec<f32> {
    if in_rate == out_rate || input.is_empty() {
        return input.to_vec();
    }
    let ratio = out_rate as f64 / in_rate as f64;
    let out_len = ((input.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 / ratio;
        let idx = src.floor() as usize;
        let frac = (src - idx as f64) as f32;
        let a = input.get(idx).copied().unwrap_or(0.0);
        let b = input.get(idx + 1).copied().unwrap_or(a);
        out.push(a + (b - a) * frac);
    }
    out
}

/// f32 [-1,1] → 16-bit PCM mono 16kHz WAV bytes。
fn encode_wav_16k(samples: &[f32]) -> Result<Vec<u8>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: TARGET_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::<u8>::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|e| anyhow!("WAV 初始化失敗: {e}"))?;
        for &s in samples {
            let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            writer
                .write_sample(v)
                .map_err(|e| anyhow!("寫入 WAV 失敗: {e}"))?;
        }
        writer.finalize().map_err(|e| anyhow!("WAV finalize 失敗: {e}"))?;
    }
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 產生指定長度、指定振幅的方波（每個取樣點正負交替），用來模擬「有聲音」的片段。
    /// 振幅即該片段的 RMS，方便測試直接對照門檻值。
    fn tone(len: usize, amplitude: f32) -> Vec<f32> {
        (0..len)
            .map(|i| if i % 2 == 0 { amplitude } else { -amplitude })
            .collect()
    }

    #[test]
    fn is_silent_returns_true_for_all_zero_samples() {
        let samples = vec![0.0f32; 16_000]; // 1 秒全靜音
        assert!(is_silent(&samples, 16_000));
    }

    /// 實測回報（2026-08-16，同日第二次調整）：門檻 0.001 時「完全沒出聲」的錄音仍被送去辨識
    /// 而生出文字，代表這台麥克風的閒置底噪高於 0.001。底噪等級（RMS 0.002）必須算沒說話。
    /// 本測試與 `is_silent_returns_false_for_speech_on_a_low_gain_microphone`（0.005 必須算有說話）
    /// 一起把門檻夾在 [0.002, 0.005) 之間——兩者是這個門檻的上下界，改動時請同時確認。
    #[test]
    fn is_silent_returns_true_for_idle_microphone_hiss() {
        let samples = tone(16_000, 0.002);
        assert!(is_silent(&samples, 16_000));
    }

    #[test]
    fn is_silent_returns_true_for_low_level_room_noise() {
        // 底噪等級（RMS 0.0003，約 -70 dBFS）應視為沒說話。
        let samples = tone(16_000, 0.0003);
        assert!(is_silent(&samples, 16_000));
    }

    /// 實測回報（2026-08-16）：門檻 0.01 時正常說話被誤判為沒聲音，要「超級大聲」才過得了關，
    /// 代表輸入裝置的電平偏低。低電平麥克風上的正常說話（RMS 0.005，約 -46 dBFS）必須算有說話。
    #[test]
    fn is_silent_returns_false_for_speech_on_a_low_gain_microphone() {
        let samples = tone(16_000, 0.005);
        assert!(!is_silent(&samples, 16_000));
    }

    #[test]
    fn is_silent_returns_false_for_normal_speech_level() {
        // 一般說話音量（RMS 0.1，約 -20 dBFS）。
        let samples = tone(16_000, 0.1);
        assert!(!is_silent(&samples, 16_000));
    }

    /// 這是逐視窗判斷的關鍵：整段大多是靜音，只有中間短短一下有講話，
    /// 若用整段平均會被稀釋成靜音而誤殺，逐視窗則能抓到。
    #[test]
    fn is_silent_returns_false_when_a_short_burst_sits_in_a_long_quiet_recording() {
        let mut samples = vec![0.0f32; 32_000]; // 2 秒
        let burst = tone(480, 0.3); // 30ms 說話
        samples[16_000..16_480].copy_from_slice(&burst);
        assert!(!is_silent(&samples, 16_000));
    }

    /// 供調參用的實測值：必須反映「最大聲的那個視窗」，不能被整段靜音稀釋成平均值。
    #[test]
    fn max_window_rms_reports_the_loudest_window_not_the_average() {
        let mut samples = vec![0.0f32; 32_000];
        // 對齊視窗邊界（16_320 = 34 × 480），讓該視窗完整落在爆音上，期望值才好對照。
        samples[16_320..16_800].copy_from_slice(&tone(480, 0.3));
        let peak = max_window_rms(&samples, 16_000);
        assert!(
            (peak - 0.3).abs() < 1e-4,
            "應回報最大聲視窗的 RMS 0.3，實際 {peak}"
        );
    }

    #[test]
    fn max_window_rms_is_zero_for_empty_samples() {
        assert_eq!(max_window_rms(&[], 16_000), 0.0);
    }

    #[test]
    fn is_silent_returns_true_for_empty_samples() {
        assert!(is_silent(&[], 16_000));
    }

    /// 錄音長度短於一個視窗時，整段當成一個視窗判斷，不可漏判。
    #[test]
    fn is_silent_handles_recording_shorter_than_one_window() {
        let samples = tone(100, 0.2);
        assert!(!is_silent(&samples, 16_000));
    }
}
