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
/// 靜音判定的視窗長度（毫秒）。夠短才抓得到很簡短的一句話。
const SILENCE_WINDOW_MS: usize = 30;
/// 說話必須高出底噪的倍數，超過才算有人講話。4 倍約等於 12 dB。
///
/// 這是「相對」門檻而非絕對音量，原因是 2026-08-16 用絕對門檻連調三次都不可靠：
/// Windows 的麥克風音量滑桿與麥克風增強、不同麥克風的靈敏度、講話距離，都會把訊號
/// 整體乘上一個倍率，使得任何絕對數值只對「某一台機器的某一組設定」成立。
/// 改看比值後，整體增益會同時放大說話與底噪、比值不變，判定因此與音量設定無關。
const SPEECH_OVER_FLOOR_RATIO: f32 = 4.0;
/// 估計底噪時取的百分位數。取偏低的分位數，才不會被說話聲本身墊高。
const NOISE_FLOOR_PERCENTILE: f32 = 0.20;
/// 絕對安全下限（RMS，約 -74 dBFS）：峰值低於此值一律視為沒說話，不看比值。
///
/// 純比值在「幾乎全零、偶爾一個極微弱尖點」時會算出很大的比值而誤判成有說話
/// （麥克風被系統靜音時只剩量化雜訊就是這種形狀）。這個下限只用來擋掉那種訊號，
/// 訂得極低以免又變回會誤殺真實語音的絕對門檻。
const MIN_SPEECH_PEAK_RMS: f32 = 0.0002;

/// 一段錄音的音量輪廓：最大聲的視窗，以及估計出來的背景底噪水準。
#[derive(Debug, Clone, Copy)]
pub struct LevelProfile {
    /// 最大聲視窗的 RMS。
    pub peak: f32,
    /// 底噪水準（視窗 RMS 的第 `NOISE_FLOOR_PERCENTILE` 百分位數）。
    pub floor: f32,
}

impl LevelProfile {
    /// 說話音量相對於底噪的倍數，即判定所依據的值。底噪為 0 時回傳極大值。
    pub fn ratio(&self) -> f32 {
        self.peak / self.floor.max(1e-9)
    }

    /// 這段錄音是否從頭到尾都沒有人說話。
    pub fn is_silent(&self) -> bool {
        self.peak < MIN_SPEECH_PEAK_RMS || self.ratio() < SPEECH_OVER_FLOOR_RATIO
    }
}

/// 算出整段錄音的音量輪廓：切成 `SILENCE_WINDOW_MS` 毫秒的視窗逐一求 RMS，
/// 取最大值當峰值、取偏低的百分位數當底噪。
///
/// 判定與 log 都以這一份結果為準，樣本只掃一趟。視窗切得夠短（30ms），
/// 長錄音裡只講了短短一句時峰值仍抓得到，不會被大量靜音稀釋。
/// 視窗數不足 2 個（錄音短於一個視窗）時無從比較起伏，回傳 `floor == peak`，
/// 依比值規則即判定為沒說話——30ms 裝不下一個完整音節，不會誤殺真正的語音。
pub fn analyze_levels(samples: &[f32], sample_rate: u32) -> LevelProfile {
    let window = ((sample_rate as usize * SILENCE_WINDOW_MS) / 1000).max(1);
    let mut windows: Vec<f32> = samples
        .chunks(window)
        .map(|w| {
            let sum_sq: f32 = w.iter().map(|s| s * s).sum();
            (sum_sq / w.len() as f32).sqrt()
        })
        .collect();
    if windows.is_empty() {
        return LevelProfile {
            peak: 0.0,
            floor: 0.0,
        };
    }
    let peak = windows.iter().copied().fold(0.0f32, f32::max);
    // 只需要第 k 小的值，用 select_nth 就好，不必整個排序。
    let k = ((windows.len() as f32 * NOISE_FLOOR_PERCENTILE) as usize).min(windows.len() - 1);
    windows.select_nth_unstable_by(k, |a, b| a.total_cmp(b));
    LevelProfile {
        peak,
        floor: windows[k],
    }
}

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
        // 實測值一律印出（不論判定結果），供日後調整判定參數時對照真實錄音。
        let levels = analyze_levels(&samples, sample_rate);
        println!(
            "[audio] 音量輪廓：峰值 RMS {:.5}／底噪 {:.5}／比值 {:.1} 倍（需 > {SPEECH_OVER_FLOOR_RATIO} 倍才算有說話）",
            levels.peak,
            levels.floor,
            levels.ratio()
        );
        if levels.is_silent() {
            println!("[audio] 音量起伏與底噪無異，判定為未說話，跳過辨識");
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

    const RATE: u32 = 16_000;

    /// 固定種子的偽亂數（線性同餘），讓測試資料每次都一樣、不會偶發失敗。
    fn lcg(seed: &mut u32) -> f32 {
        *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (*seed >> 8) as f32 / 8_388_608.0 - 1.0 // [-1, 1)
    }

    /// 穩定的背景底噪（冷氣、風扇、電源嗡嗡聲）：音量從頭到尾大致不變。
    fn steady_noise(len: usize, level: f32) -> Vec<f32> {
        let mut seed = 12_345;
        (0..len).map(|_| lcg(&mut seed) * level).collect()
    }

    /// 有音節起伏的語音：每 300ms 一個音節，其中 180ms 出聲、120ms 是字間空隙。
    /// 這個「大小聲交替」正是比值做法用來跟穩定底噪區分的特徵。
    fn speech_like(len: usize, level: f32) -> Vec<f32> {
        let mut seed = 999;
        (0..len)
            .map(|i| {
                let voiced = (i % 4_800) < 2_880;
                let amp = if voiced { level } else { level / 50.0 };
                lcg(&mut seed) * amp
            })
            .collect()
    }

    fn silent(samples: &[f32]) -> bool {
        analyze_levels(samples, RATE).is_silent()
    }

    fn amplified(samples: &[f32], gain: f32) -> Vec<f32> {
        samples.iter().map(|s| s * gain).collect()
    }

    /// **這個做法的核心保證**：Windows 的麥克風音量滑桿／麥克風增強只是把訊號整體乘上一個倍率，
    /// 說話與底噪會被同時放大，因此判斷結果必須完全不受影響。絕對門檻做不到這件事——
    /// 這正是 2026-08-16 反覆調參三次仍不可靠的根本原因。
    #[test]
    fn verdict_is_unchanged_when_the_whole_recording_is_amplified() {
        let speech = speech_like(RATE as usize * 2, 0.005);
        let noise = steady_noise(RATE as usize * 2, 0.002);
        for gain in [0.25, 1.0, 4.0, 10.0] {
            assert!(
                !silent(&amplified(&speech, gain)),
                "放大 {gain} 倍後語音仍必須被判定為有說話"
            );
            assert!(
                silent(&amplified(&noise, gain)),
                "放大 {gain} 倍後底噪仍必須被判定為沒說話"
            );
        }
    }

    /// 穩定底噪不論多大聲都算沒說話：使用者環境有多吵、麥克風增益開多大都不該影響判斷。
    /// 絕對門檻在這裡必然失敗——0.02 的底噪遠高於任何合理的絕對門檻。
    #[test]
    fn steady_background_noise_is_silent_regardless_of_its_level() {
        for level in [0.0005, 0.002, 0.02, 0.2] {
            let samples = steady_noise(RATE as usize * 2, level);
            assert!(silent(&samples), "音量 {level} 的穩定底噪應判定為沒說話");
        }
    }

    /// 反過來，語音不論多小聲都要被認出來（前提是高於數位靜音的安全下限）。
    #[test]
    fn speech_is_detected_across_a_wide_range_of_levels() {
        for level in [0.002, 0.005, 0.05, 0.3] {
            let samples = speech_like(RATE as usize * 2, level);
            assert!(!silent(&samples), "音量 {level} 的語音應判定為有說話");
        }
    }

    #[test]
    fn all_zero_samples_are_silent() {
        assert!(silent(&vec![0.0f32; RATE as usize]));
    }

    /// 安全下限的用途：麥克風被系統靜音時只剩極微弱的量化雜訊，這種訊號的「起伏比值」
    /// 可能很大（幾乎全零，偶爾一個尖點），光看比值會誤判成有說話，故另設絕對下限擋掉。
    #[test]
    fn barely_perceptible_dither_spikes_are_silent() {
        let mut samples = vec![0.0f32; RATE as usize];
        for i in (0..samples.len()).step_by(1_000) {
            samples[i] = 0.0001;
        }
        assert!(silent(&samples));
    }

    /// 整段大多安靜、只有中間短短一下有講話，仍必須判定為有說話
    /// （長錄音裡的短句不可被大量靜音稀釋而誤殺）。
    #[test]
    fn a_short_burst_in_a_long_quiet_recording_is_not_silent() {
        let mut samples = vec![0.0f32; RATE as usize * 2];
        let burst = steady_noise(480, 0.3);
        samples[16_000..16_480].copy_from_slice(&burst);
        assert!(!silent(&samples));
    }

    #[test]
    fn empty_samples_are_silent() {
        assert!(silent(&[]));
    }

    /// 短於一個視窗（30ms）的錄音無從比較起伏，一律當成沒說話。
    /// 30ms 裝不下一個完整音節，這樣處理不會誤殺真正的語音。
    #[test]
    fn recording_shorter_than_one_window_is_silent() {
        assert!(silent(&steady_noise(100, 0.2)));
    }

    /// 供 log 與日後調參使用：峰值取最大聲的視窗，底噪取第 20 百分位數。
    #[test]
    fn analyze_levels_reports_peak_and_noise_floor() {
        let mut samples = vec![0.02f32; RATE as usize];
        // 對齊視窗邊界（4_800 = 10 × 480），讓該視窗完整落在爆音上，期望值才好對照。
        samples[4_800..5_280].fill(0.5);
        let levels = analyze_levels(&samples, RATE);
        assert!(
            (levels.peak - 0.5).abs() < 1e-4,
            "峰值應為最大聲視窗的 0.5，實際 {}",
            levels.peak
        );
        assert!(
            (levels.floor - 0.02).abs() < 1e-4,
            "底噪應為 0.02，實際 {}",
            levels.floor
        );
    }
}
