//! 錄音與 WAV 封裝（規格第 4 節）。
//! cpal 抓麥克風到記憶體 buffer（混為單聲道），停止時降取樣到 16kHz 並用 hound 封裝成 WAV bytes，
//! 全程不落地。Recorder 持有 cpal::Stream（!Send），僅在 controller 執行緒內使用。

use anyhow::{anyhow, bail, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

const TARGET_RATE: u32 = 16_000;
const MAX_WAV_BYTES: usize = 25 * 1024 * 1024; // Groq 25MB 上限
/// 靜音判定的視窗長度（毫秒）。夠短才抓得到很簡短的一句話。
const SILENCE_WINDOW_MS: usize = 30;
/// 說話必須高出底噪的倍數，超過才算有人講話。3 倍約等於 9.5 dB。
///
/// 這是「相對」門檻而非絕對音量，原因是 2026-08-16 用絕對門檻連調三次都不可靠：
/// Windows 的麥克風音量滑桿與麥克風增強、不同麥克風的靈敏度、講話距離，都會把訊號
/// 整體乘上一個倍率，使得任何絕對數值只對「某一台機器的某一組設定」成立。
/// 改看比值後，整體增益會同時放大說話與底噪、比值不變，判定因此與音量設定無關。
///
/// 倍數由 4.0 放寬到 3.0（使用者要求，讓人不必刻意大聲講話）。實測參考值：
/// 不出聲 2.3 倍、正常說話 5.9 倍。放寬後距離「不出聲」只剩約 2.3 dB，
/// 單靠倍數已不足以擋掉突發雜音，故**同時**加上 `MIN_SPEECH_WINDOWS` 的持續時間條件。
const SPEECH_OVER_FLOOR_RATIO: f32 = 3.0;
/// 超過門檻的視窗要有幾個，才算真的有人在講話（4 個視窗 = 120 ms）。
///
/// 用途是擋掉單一突發雜音：敲鍵盤、椅子聲、呼吸只會佔一兩個視窗，而真正的說話
/// 就算只講一個字也會持續好幾個視窗。這個條件讓安全性由「要持續一段時間」負責，
/// 倍數門檻才有本錢放寬。訂 4 個（而非更多）是為了保留很短的單音節應答——
/// 一般人刻意講一個字通常有 150~250 ms；若實測發現短應答被吃掉，可再往下調。
const MIN_SPEECH_WINDOWS: usize = 4;
/// 估計底噪時取的百分位數。取偏低的分位數，才不會被說話聲本身墊高。
const NOISE_FLOOR_PERCENTILE: f32 = 0.20;
/// 絕對安全下限（RMS，約 -74 dBFS）：峰值低於此值一律視為沒說話，不看比值。
///
/// 純比值在「幾乎全零、偶爾一個極微弱尖點」時會算出很大的比值而誤判成有說話
/// （麥克風被系統靜音時只剩量化雜訊就是這種形狀）。這個下限只用來擋掉那種訊號，
/// 訂得極低以免又變回會誤殺真實語音的絕對門檻。
const MIN_SPEECH_PEAK_RMS: f32 = 0.0002;

/// 一段錄音的音量輪廓：最大聲的視窗、估計出來的背景底噪水準，以及明顯高於底噪的視窗數。
#[derive(Debug, Clone, Copy)]
pub struct LevelProfile {
    /// 最大聲視窗的 RMS。
    pub peak: f32,
    /// 底噪水準（視窗 RMS 的第 `NOISE_FLOOR_PERCENTILE` 百分位數）。
    pub floor: f32,
    /// 超過 `floor × SPEECH_OVER_FLOOR_RATIO` 的視窗數，即「大聲講話持續了多久」。
    pub loud_windows: usize,
}

impl LevelProfile {
    /// 峰值相對於底噪的倍數。底噪為 0 時分母以極小值代入，回傳極大值。
    pub fn ratio(&self) -> f32 {
        self.peak / self.floor.max(1e-9)
    }

    /// 這段錄音是否從頭到尾都沒有人說話。
    ///
    /// 三個條件任一成立即判定為沒說話：
    /// 1. 峰值低於絕對安全下限（麥克風被靜音時的量化雜訊，比值無意義）。
    /// 2. 峰值不到底噪的 `SPEECH_OVER_FLOOR_RATIO` 倍（整段平坦，與底噪無異）。
    /// 3. 超過門檻的視窗不足 `MIN_SPEECH_WINDOWS` 個（只是突發雜音，沒有持續講話）。
    pub fn is_silent(&self) -> bool {
        self.peak < MIN_SPEECH_PEAK_RMS
            || self.ratio() < SPEECH_OVER_FLOOR_RATIO
            || self.loud_windows < MIN_SPEECH_WINDOWS
    }
}

/// 切成 `SILENCE_WINDOW_MS` 毫秒的視窗，逐一求 RMS。
pub fn window_rms(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    let window = ((sample_rate as usize * SILENCE_WINDOW_MS) / 1000).max(1);
    samples
        .chunks(window)
        .map(|w| {
            let sum_sq: f32 = w.iter().map(|s| s * s).sum();
            (sum_sq / w.len() as f32).sqrt()
        })
        .collect()
}

/// 由視窗 RMS 清單算出音量輪廓：取最大值當峰值、取偏低的百分位數當底噪。
///
/// 停止錄音時的靜音略過與錄音中的即時警告（`live_warning`）都走這一個函式，
/// 兩邊的判準因此不會各自漂移。視窗切得夠短（30ms），
/// 長錄音裡只講了短短一句時峰值仍抓得到，不會被大量靜音稀釋。
/// 視窗數不足 2 個（錄音短於一個視窗）時無從比較起伏，回傳 `floor == peak`，
/// 依比值規則即判定為沒說話——30ms 裝不下一個完整音節，不會誤殺真正的語音。
pub fn profile_from_windows(mut windows: Vec<f32>) -> LevelProfile {
    if windows.is_empty() {
        return LevelProfile {
            peak: 0.0,
            floor: 0.0,
            loud_windows: 0,
        };
    }
    let peak = windows.iter().copied().fold(0.0f32, f32::max);
    // 只需要第 k 小的值，用 select_nth 就好，不必整個排序。
    let k = ((windows.len() as f32 * NOISE_FLOOR_PERCENTILE) as usize).min(windows.len() - 1);
    windows.select_nth_unstable_by(k, |a, b| a.total_cmp(b));
    let floor = windows[k];
    // select_nth 只做部分重排，不影響「數有幾個超過門檻」。
    let cutoff = floor.max(1e-9) * SPEECH_OVER_FLOOR_RATIO;
    let loud_windows = windows.iter().filter(|&&w| w > cutoff).count();
    LevelProfile {
        peak,
        floor,
        loud_windows,
    }
}

/// 錄音最前面這麼久不計入「有沒有說話」的判斷。
///
/// 開始錄音的提示音（`sound::play_start`）會被筆電的麥克風收進去，聽起來就是一段很明顯的聲音，
/// 會被誤當成說話：警告因此不出現，停止時也會把沒講話的錄音送去辨識（得到「辨識結果為空」）。
/// 使用者按下熱鍵到真正開口通常也超過這個時間；代價是按下熱鍵 0.7 秒內就結束的極短錄音會被略過。
pub const START_SKIP_MS: usize = 700;
/// `START_SKIP_MS` 折合的視窗數。
const START_SKIP_WINDOWS: usize = START_SKIP_MS / SILENCE_WINDOW_MS;

/// 去掉錄音最前面不計入判斷的視窗（錄音比這更短就整段都去掉）。
fn skip_start(windows: &[f32]) -> &[f32] {
    &windows[START_SKIP_WINDOWS.min(windows.len())..]
}

/// 算出整段錄音的音量輪廓（`window_rms` ＋ 略過開頭 ＋ `profile_from_windows`）。
pub fn analyze_levels(samples: &[f32], sample_rate: u32) -> LevelProfile {
    profile_from_windows(skip_start(&window_rms(samples, sample_rate)).to_vec())
}

/// 把大聲區段排成 log 用的文字，例如「1.5–1.8s, 5.4–5.5s」；沒有就寫「（無）」，太多只列前 10 段。
fn format_segments(segments: &[(f32, f32)]) -> String {
    if segments.is_empty() {
        return "（無）".to_string();
    }
    let mut text = segments
        .iter()
        .take(10)
        .map(|(a, b)| format!("{a:.1}–{b:.1}s"))
        .collect::<Vec<_>>()
        .join(", ");
    if segments.len() > 10 {
        text.push_str(&format!(" …共 {} 段", segments.len()));
    }
    text
}

/// 逐視窗標記「明顯大聲」：高於底噪（第 20 百分位數）的 `SPEECH_OVER_FLOOR_RATIO` 倍，
/// 且不低於絕對安全下限。跟 `profile_from_windows` 用同一個底噪與倍數，只是不再只數總數。
fn loud_flags(analyzed: &[f32]) -> Vec<bool> {
    let floor = profile_from_windows(analyzed.to_vec()).floor;
    let cutoff = floor.max(1e-9) * SPEECH_OVER_FLOOR_RATIO;
    analyzed
        .iter()
        .map(|&w| w > cutoff && w >= MIN_SPEECH_PEAK_RMS)
        .collect()
}

/// 明顯大聲的區段（錄音的第幾秒到第幾秒），供停止錄音時印 log、日後調參對照用。
/// 已略過開頭 `START_SKIP_MS`，所以那段不會出現在結果裡。
pub fn loud_segments(windows: &[f32]) -> Vec<(f32, f32)> {
    let analyzed = skip_start(windows);
    let offset = windows.len() - analyzed.len();
    let to_secs = |idx: usize| (offset + idx) as f32 * SILENCE_WINDOW_MS as f32 / 1000.0;
    let mut segments = Vec::new();
    let mut start: Option<usize> = None;
    for (i, loud) in loud_flags(analyzed).into_iter().enumerate() {
        match (loud, start) {
            (true, None) => start = Some(i),
            (false, Some(st)) => {
                segments.push((to_secs(st), to_secs(i)));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(st) = start {
        segments.push((to_secs(st), to_secs(analyzed.len())));
    }
    segments
}

/// 「2 秒」折合的視窗數：錄音滿這麼久才開始判斷警告，「收不到聲音」也只看最近這麼多視窗。
pub const WARN_WINDOWS: usize = 2000 / SILENCE_WINDOW_MS;

/// 錄音中的即時音量警告，浮動視窗據此顯示提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LiveWarning {
    /// 沒有問題（或還太早，無從判斷）。
    None,
    /// 聲音太小：最近一段時間沒有聽到明顯的說話（多半是離麥克風太遠），見 `live_warning`。
    Low,
    /// 收不到聲音：最近 2 秒幾乎全零（麥克風被關掉或沒有訊號）。
    Muted,
}

/// 還沒聽到說話時，安靜多久才提醒「聲音太小」。
const WAIT_BEFORE_SPEECH_WINDOWS: usize = 4000 / SILENCE_WINDOW_MS;
/// 已經講過話之後，要「連續安靜」多久才提醒（說話中間停頓個幾秒很正常，放寬很多）。
const WAIT_AFTER_SPEECH_WINDOWS: usize = 8000 / SILENCE_WINDOW_MS;
/// 判斷「一陣說話」的時間跨度：這麼久之內要有 `MIN_SPEECH_WINDOWS` 個明顯大聲的視窗。
/// 限定在 1 秒內，才不會讓分散在整段錄音裡的零碎雜音（呼吸、按鍵、椅子聲）累積成「有說話」。
const BURST_SPAN_WINDOWS: usize = 1000 / SILENCE_WINDOW_MS;

/// 依「錄音開始至今」的視窗 RMS 清單判斷目前該顯示哪種警告。
///
/// 規則（照順序）：
/// 1. 未滿 2 秒不判斷。
/// 2. 最近 2 秒每個視窗都低於絕對下限 → `Muted`（紅）。
/// 3. 略過開頭 `START_SKIP_MS` 後，分析的部分未滿 4 秒 → `None`。
/// 4. 找出最後一次「一陣說話」（1 秒內 ≥ 4 個明顯大聲的視窗，見 `BURST_SPAN_WINDOWS`）：
///    從沒說過話 → 安靜滿 4 秒就是 `Low`；說過話 → 要從那之後連續安靜滿 8 秒才是 `Low`。
///    再開口說話，警告馬上清掉，之後又沉默才重新計時。
///
/// 全部以相對於底噪的倍數判斷（沿用靜音略過的標準），不受 Windows 麥克風音量滑桿影響。
/// 這個警告看的是「最近安靜了多久」，**不再等於**「停止時會被略過」（那個看整段錄音）。
pub fn live_warning(windows: &[f32]) -> LiveWarning {
    if windows.len() < WARN_WINDOWS {
        return LiveWarning::None;
    }
    if windows[windows.len() - WARN_WINDOWS..]
        .iter()
        .all(|&w| w < MIN_SPEECH_PEAK_RMS)
    {
        return LiveWarning::Muted;
    }
    let analyzed = skip_start(windows);
    if analyzed.len() < WAIT_BEFORE_SPEECH_WINDOWS {
        return LiveWarning::None;
    }
    let loud = loud_flags(analyzed);
    // 前綴和：prefix[i] = 前 i 個視窗裡明顯大聲的數量，用來 O(1) 查任一段的數量。
    let mut prefix = vec![0usize; loud.len() + 1];
    for (i, &l) in loud.iter().enumerate() {
        prefix[i + 1] = prefix[i] + l as usize;
    }
    let last_burst = (0..loud.len()).rev().find(|&i| {
        loud[i] && prefix[i + 1] - prefix[(i + 1).saturating_sub(BURST_SPAN_WINDOWS)] >= MIN_SPEECH_WINDOWS
    });
    let (quiet, limit) = match last_burst {
        Some(i) => (loud.len() - 1 - i, WAIT_AFTER_SPEECH_WINDOWS),
        None => (loud.len(), WAIT_BEFORE_SPEECH_WINDOWS),
    };
    if quiet >= limit {
        LiveWarning::Low
    } else {
        LiveWarning::None
    }
}

/// 浮動視窗波形的條數。
pub const BAR_COUNT: usize = 15;
/// 波形取最近多少毫秒的樣本來分段。
const TAIL_MS: usize = 40;

/// 把樣本平均切成 `n` 段、各算一個 RMS。永遠回傳 `n` 個值；樣本不足時空的段為 0.0。
pub fn segment_rms(samples: &[f32], n: usize) -> Vec<f32> {
    let len = samples.len();
    (0..n)
        .map(|i| {
            let seg = &samples[i * len / n..(i + 1) * len / n];
            if seg.is_empty() {
                0.0
            } else {
                let sum_sq: f32 = seg.iter().map(|s| s * s).sum();
                (sum_sq / seg.len() as f32).sqrt()
            }
        })
        .collect()
}

struct MeterInner {
    /// 一個 `SILENCE_WINDOW_MS` 視窗折合的樣本數（依裝置取樣率）。
    window_len: usize,
    /// 尾段最多保留的樣本數（`TAIL_MS` 毫秒份量）。
    tail_cap: usize,
    tail: VecDeque<f32>,
    /// 錄音開始以來每個完整視窗的 RMS。
    windows: Vec<f32>,
    /// 還沒湊滿一個視窗的平方和與樣本數。
    acc_sum_sq: f32,
    acc_count: usize,
}

impl MeterInner {
    fn empty(sample_rate: u32) -> Self {
        let rate = sample_rate as usize;
        MeterInner {
            window_len: (rate * SILENCE_WINDOW_MS / 1000).max(1),
            tail_cap: (rate * TAIL_MS / 1000).max(1),
            tail: VecDeque::new(),
            windows: Vec::new(),
            acc_sum_sq: 0.0,
            acc_count: 0,
        }
    }
}

/// 錄音中的即時音量計：音訊 callback 餵樣本進來，浮動視窗取 15 段波形與警告。
///
/// 只存在記憶體，`reset`（開始錄音）與 `clear`（停止錄音）都會把樣本清空，
/// 符合「語音資料用完即丟」的規則。內部用 `Mutex` 保護；呼叫端取值後再去呼叫
/// Tauri API，不要在持有任何鎖時 emit。
pub struct Meter {
    inner: Mutex<MeterInner>,
}

impl Default for Meter {
    fn default() -> Self {
        Self::new()
    }
}

impl Meter {
    pub fn new() -> Self {
        Meter {
            inner: Mutex::new(MeterInner::empty(TARGET_RATE)),
        }
    }

    /// 開始新一段錄音：清空並依裝置取樣率重算視窗與尾段長度。
    pub fn reset(&self, sample_rate: u32) {
        *self.inner.lock().unwrap() = MeterInner::empty(sample_rate);
    }

    /// 餵入單聲道樣本。
    pub fn push(&self, mono: &[f32]) {
        let mut m = self.inner.lock().unwrap();
        for &s in mono {
            if m.tail.len() >= m.tail_cap {
                m.tail.pop_front();
            }
            m.tail.push_back(s);
            m.acc_sum_sq += s * s;
            m.acc_count += 1;
            if m.acc_count >= m.window_len {
                let rms = (m.acc_sum_sq / m.acc_count as f32).sqrt();
                m.windows.push(rms);
                m.acc_sum_sq = 0.0;
                m.acc_count = 0;
            }
        }
    }

    /// 最近約 40ms 的樣本分成 `BAR_COUNT` 段的 RMS（原始值，未正規化）。
    pub fn bars(&self) -> Vec<f32> {
        let tail: Vec<f32> = self.inner.lock().unwrap().tail.iter().copied().collect();
        segment_rms(&tail, BAR_COUNT)
    }

    /// 目前該顯示的警告。鎖內只複製視窗清單，判斷放到鎖外做。
    pub fn warning(&self) -> LiveWarning {
        let windows = self.inner.lock().unwrap().windows.clone();
        live_warning(&windows)
    }

    /// 清空所有樣本與視窗（停止錄音時呼叫）。
    pub fn clear(&self) {
        let mut m = self.inner.lock().unwrap();
        let rate = (m.window_len * 1000 / SILENCE_WINDOW_MS) as u32;
        *m = MeterInner::empty(rate);
    }
}

/// 被丟掉時清空即時音量計。`Recorder` 持有它，所以不論錄音是正常結束（`stop_to_wav`）、
/// 還是中途失敗被直接丟掉（例如沒設定 STT 金鑰時 `stop_to_wav` 根本沒被呼叫），
/// 樣本都不會留到下一次錄音——符合「語音資料用完即丟」。
struct MeterGuard(Arc<Meter>);

impl Drop for MeterGuard {
    fn drop(&mut self) {
        self.0.clear();
    }
}

pub struct Recorder {
    // 欄位宣告順序即丟棄順序：先停串流、再清音量計，callback 不會在清空後又塞進樣本。
    stream: cpal::Stream,
    buf: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    meter: MeterGuard,
}

/// 開始錄音：開啟預設輸入裝置的串流，PCM 持續寫入記憶體 buffer。
/// `meter` 由本函式的音訊 callback 持續餵入單聲道樣本，供浮動視窗取波形與警告；開始時先 reset。
pub fn start(meter: Arc<Meter>) -> Result<Recorder> {
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

    meter.reset(sample_rate);
    let buf = Arc::new(Mutex::new(Vec::<f32>::new()));
    let buf_cb = buf.clone();
    let meter_cb = meter.clone();
    let err_fn = |e| eprintln!("[audio] 串流錯誤: {e}");

    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _: &_| push_mono(&buf_cb, data, channels, &meter_cb),
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _: &_| {
                let f: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                push_mono(&buf_cb, &f, channels, &meter_cb);
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config,
            move |data: &[u16], _: &_| {
                let f: Vec<f32> = data.iter().map(|&s| (s as f32 - 32768.0) / 32768.0).collect();
                push_mono(&buf_cb, &f, channels, &meter_cb);
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
        meter: MeterGuard(meter),
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
            meter,
        } = self;
        drop(stream); // 停止擷取
        meter.0.clear(); // 即時音量資料用完即丟（其餘離開路徑由 MeterGuard 的 Drop 負責）

        let samples = std::mem::take(&mut *buf.lock().unwrap());
        if samples.is_empty() {
            bail!("沒有錄到任何音訊");
        }
        // 實測值一律印出（不論判定結果），供日後調整判定參數時對照真實錄音。
        let levels = analyze_levels(&samples, sample_rate);
        println!(
            "[audio] 音量輪廓：峰值 RMS {:.5}／底噪 {:.5}／比值 {:.1} 倍（需 > {SPEECH_OVER_FLOOR_RATIO}）／超標視窗 {} 個 ≈ {} ms（需 ≥ {MIN_SPEECH_WINDOWS} 個）",
            levels.peak,
            levels.floor,
            levels.ratio(),
            levels.loud_windows,
            levels.loud_windows * SILENCE_WINDOW_MS
        );
        println!(
            "[audio] 明顯大聲的區段（已略過開頭 {START_SKIP_MS}ms）：{}",
            format_segments(&loud_segments(&window_rms(&samples, sample_rate)))
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

/// 把交錯的多聲道資料混成單聲道後追加到 buffer，同時餵給即時音量計。
fn push_mono(buf: &Arc<Mutex<Vec<f32>>>, data: &[f32], channels: usize, meter: &Meter) {
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
    meter.push(&b[start..]);
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

    /// 整段大多安靜、只有中間短短一句話，仍必須判定為有說話
    /// （長錄音裡的短句不可被大量靜音稀釋而誤殺）。150ms ≈ 5 個視窗。
    #[test]
    fn a_short_utterance_in_a_long_quiet_recording_is_not_silent() {
        let mut samples = vec![0.0f32; RATE as usize * 2];
        let utterance = steady_noise(2_400, 0.3);
        // 要放在略過的開頭（START_SKIP_MS = 700ms）之後，從 1 秒處開始。
        samples[16_000..18_400].copy_from_slice(&utterance);
        assert!(!silent(&samples));
    }

    /// 單一突發雜音（敲鍵盤、椅子聲、呼吸）只會佔一兩個視窗，不該算成說話。
    /// 這是把倍數門檻放寬到 3 倍的前提：安全性改由「要持續一段時間」負責，
    /// 而不是靠倍數訂高——否則不出聲的錄音只要有人碰一下桌子就會被送去辨識。
    #[test]
    fn an_isolated_click_is_not_treated_as_speech() {
        let mut samples = steady_noise(RATE as usize * 2, 0.002);
        let click = steady_noise(480, 0.2); // 30ms，僅一個視窗
        samples[8_000..8_480].copy_from_slice(&click);
        assert!(silent(&samples));
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
        // 對齊視窗邊界（11_520 = 24 × 480），讓該視窗完整落在爆音上，期望值才好對照；
        // 必須落在略過的開頭（START_SKIP_MS）之後，否則不會被計入。
        samples[11_520..12_000].fill(0.5); // 第 24 個視窗（約 0.72 秒），略過開頭 0.7 秒之後的第一個
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

    // ---- 錄音中即時警告（見 live_warning）----

    fn warn(samples: &[f32]) -> LiveWarning {
        live_warning(&window_rms(samples, RATE))
    }

    #[test]
    fn no_warning_before_two_seconds() {
        assert_eq!(warn(&vec![0.0; RATE as usize]), LiveWarning::None);
        assert_eq!(warn(&steady_noise(RATE as usize * 3 / 2, 0.002)), LiveWarning::None);
    }

    #[test]
    fn all_zero_for_two_seconds_is_muted() {
        assert_eq!(warn(&vec![0.0; RATE as usize * 3]), LiveWarning::Muted);
    }

    #[test]
    fn steady_noise_is_low_regardless_of_level() {
        for level in [0.0005, 0.002, 0.02, 0.2] {
            assert_eq!(
                warn(&steady_noise(RATE as usize * 6, level)),
                LiveWarning::Low,
                "level {level}"
            );
        }
    }

    #[test]
    fn speech_has_no_warning_across_levels() {
        for level in [0.002, 0.005, 0.05, 0.3] {
            assert_eq!(
                warn(&speech_like(RATE as usize * 6, level)),
                LiveWarning::None,
                "level {level}"
            );
        }
    }

    #[test]
    fn pausing_after_speech_stays_clear() {
        let mut s = speech_like(RATE as usize, 0.02);
        // RMS 約 0.00058，明顯高於 MIN_SPEECH_PEAK_RMS，不會落入 Muted。
        s.extend(steady_noise(RATE as usize * 3, 0.001));
        assert_eq!(warn(&s), LiveWarning::None);
    }

    #[test]
    fn muting_mid_recording_turns_red_then_recovers() {
        let mut s = speech_like(RATE as usize * 2, 0.02);
        s.extend(vec![0.0; RATE as usize * 2 + 1_000]);
        assert_eq!(warn(&s), LiveWarning::Muted);
        s.extend(speech_like(RATE as usize / 2, 0.02));
        assert_eq!(warn(&s), LiveWarning::None);
    }

    #[test]
    fn live_warning_is_unchanged_when_amplified() {
        for base in [
            steady_noise(RATE as usize * 6, 0.004),
            speech_like(RATE as usize * 6, 0.003),
        ] {
            let v = warn(&base);
            for gain in [0.25, 4.0, 20.0] {
                assert_eq!(warn(&amplified(&base, gain)), v, "gain {gain}");
            }
        }
    }

    // ---- 即時音量計 Meter ----

    #[test]
    fn segment_rms_always_returns_n_values() {
        assert_eq!(segment_rms(&[], BAR_COUNT), vec![0.0; BAR_COUNT]);
        assert_eq!(segment_rms(&[0.5; 10], BAR_COUNT).len(), BAR_COUNT);
        let v = segment_rms(&[0.5; 1_000], BAR_COUNT);
        assert!(v.iter().all(|x| (x - 0.5).abs() < 1e-6));
    }

    #[test]
    fn meter_windows_follow_device_sample_rate() {
        let m = Meter::new();
        m.reset(44_100);
        for chunk in vec![0.0f32; 44_100 * 3].chunks(441) {
            m.push(chunk);
        }
        assert_eq!(m.warning(), LiveWarning::Muted); // 3 秒全零，已滿 2 秒
        m.reset(44_100);
        for chunk in vec![0.0f32; 44_100].chunks(441) {
            m.push(chunk);
        }
        assert_eq!(m.warning(), LiveWarning::None); // 1 秒，未滿 2 秒
    }

    #[test]
    fn meter_bars_reflect_recent_samples_only() {
        let m = Meter::new();
        m.reset(48_000);
        m.push(&vec![0.5; 48_000]);
        m.push(&vec![0.0; 48_000 * 40 / 1000]); // 剛好一個尾段長度的靜音
        assert_eq!(m.bars().len(), BAR_COUNT);
        assert!(m.bars().iter().all(|&b| b < 1e-6));
    }

    #[test]
    fn meter_clear_drops_everything() {
        let m = Meter::new();
        m.reset(16_000);
        m.push(&vec![0.3; 16_000 * 3]);
        m.clear();
        assert!(m.bars().iter().all(|&b| b == 0.0));
        assert_eq!(m.warning(), LiveWarning::None);
    }

    /// 錄音提早失敗（例如沒設定 STT 金鑰，`stop_to_wav` 根本沒被呼叫）時，Recorder 直接被丟掉；
    /// 即時音量計必須跟著清空，不能把最後一段樣本留到下一次錄音。
    #[test]
    fn meter_guard_clears_the_meter_when_dropped() {
        let m = Arc::new(Meter::new());
        m.reset(16_000);
        m.push(&vec![0.3; 16_000 * 3]);
        assert!(m.bars().iter().any(|&b| b > 0.0), "前置：尚未清空時應有樣本");
        {
            let _guard = MeterGuard(m.clone());
        }
        assert!(m.bars().iter().all(|&b| b == 0.0));
        assert_eq!(m.warning(), LiveWarning::None);
    }

    // ---- 新規則：先等 4 秒、講過話後等 8 秒、略過開頭 0.7 秒 ----

    fn secs(n: f32) -> usize {
        (RATE as f32 * n) as usize
    }

    #[test]
    fn low_appears_after_four_quiet_seconds_when_nobody_has_spoken() {
        // 開頭 0.7 秒不計，所以總長 4.6 秒（實際分析 3.9 秒）還不提醒，5 秒才提醒。
        assert_eq!(warn(&steady_noise(secs(4.6), 0.002)), LiveWarning::None);
        assert_eq!(warn(&steady_noise(secs(5.0), 0.002)), LiveWarning::Low);
    }

    #[test]
    fn after_speech_a_pause_needs_eight_seconds_before_low() {
        let mut s = speech_like(secs(2.0), 0.02);
        s.extend(steady_noise(secs(7.0), 0.001));
        assert_eq!(warn(&s), LiveWarning::None, "停頓 7 秒還不到 8 秒");
        s.extend(steady_noise(secs(2.0), 0.001));
        assert_eq!(warn(&s), LiveWarning::Low, "停頓滿 9 秒應提醒");
    }

    #[test]
    fn speaking_again_clears_the_warning() {
        let mut s = steady_noise(secs(6.0), 0.002);
        assert_eq!(warn(&s), LiveWarning::Low);
        s.extend(speech_like(secs(1.0), 0.05));
        assert_eq!(warn(&s), LiveWarning::None);
    }

    #[test]
    fn isolated_clicks_do_not_count_as_speech() {
        let mut s = steady_noise(secs(9.0), 0.002);
        // 每 2 秒一聲短促的敲擊（約 60ms，2~3 個視窗），1 秒內湊不滿 4 個明顯大聲的視窗。
        for sec in [1.5, 3.5, 5.5, 7.5] {
            let i = secs(sec);
            for x in &mut s[i..i + 960] {
                *x = 0.3;
            }
        }
        assert_eq!(warn(&s), LiveWarning::Low);
    }

    /// 開始錄音的提示音會被筆電麥克風收進去；它不是說話，不能讓警告消失，
    /// 也不能讓「沒講話的錄音」在停止時被當成有說話而送去辨識。
    #[test]
    fn the_start_chime_in_the_first_700ms_is_ignored() {
        let mut s = vec![0.3; secs(0.5)];
        s.extend(steady_noise(secs(5.0), 0.002));
        assert_eq!(warn(&s), LiveWarning::Low);
        assert!(silent(&s), "停止時也不該把開頭的提示音當成說話");
    }

    #[test]
    fn loud_segments_reports_where_the_loud_parts_are_in_seconds() {
        let mut s = steady_noise(secs(3.0), 0.001);
        for x in &mut s[secs(1.5)..secs(1.8)] {
            *x *= 50.0;
        }
        let segs = loud_segments(&window_rms(&s, RATE));
        assert_eq!(segs.len(), 1, "{segs:?}");
        assert!((segs[0].0 - 1.5).abs() < 0.05 && (segs[0].1 - 1.8).abs() < 0.05, "{segs:?}");
    }
}
