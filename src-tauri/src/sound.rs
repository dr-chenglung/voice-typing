//! 錄音開始／結束的提示音。
//!
//! 播放**內嵌**的 WAV（`include_bytes!`），透過 `PlaySoundW` 的 `SND_MEMORY` 從記憶體
//! 播放。之所以不用系統事件音別名（`SND_ALIAS`），是因為那會依賴使用者的「音效方案」
//! 設定——若該事件被設成「(無)」就完全沒聲音（實測踩到）；內嵌固定 WAV 則不論機器
//! 設定、也不論 `C:\Windows\Media` 檔案是否存在，都保證播得出來。
//!
//! `SND_ASYNC` 立即返回、不阻塞 controller 執行緒（規格 6.1：背景工作執行緒不可卡住）。
//! 內嵌資料為 `'static`，`SND_ASYNC` 播放期間記憶體恆有效。音效屬輔助回饋，播放失敗
//! 一律靜默忽略，絕不影響主流程。
//!
//! 僅 Windows；對外只暴露 `play_start`/`play_stop` 兩個平台無關的介面。

use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY};

// 短促的純音（2026-11-16 起，參考 SayIt 的做法，再依使用者要求調得更低沉柔和、帶科技感）：
// 開始音 480→640Hz 往上滑、0.16 秒；結束音 520→360Hz 往下滑、0.14 秒（SayIt 是固定 880Hz、0.15 秒）。
// 音量 0.2 起、指數衰減到 0.001，一上一下好分辨，純正弦波不加任何雜質。刻意做短：筆電的麥克風會把提示音收進錄音，
// 越短越不容易被當成說話（見 audio.rs 的 START_SKIP_MS）。音量已在資產階段烤好（PlaySound 無音量參數）。
// 內嵌進執行檔，不依賴系統音效方案或 C:\Windows\Media 檔案是否存在。
// （2026-07-14 的舊版是 0.47 秒的兩音敲擊聲，保留在 git 歷史與 tag v0.2.3。）
static START_WAV: &[u8] = include_bytes!("../assets/start.wav");
static STOP_WAV: &[u8] = include_bytes!("../assets/stop.wav");

/// 錄音開始提示音（480→640Hz 往上滑的短促一聲）。
pub fn play_start() {
    play(START_WAV);
}

/// 錄音結束提示音（520→360Hz 往下滑的短促一聲）。
pub fn play_stop() {
    play(STOP_WAV);
}

/// 從記憶體非同步播放一段 WAV；失敗靜默忽略。
fn play(wav: &'static [u8]) {
    // SND_MEMORY：第一參數為指向記憶體中 WAV 資料的指標（型別上仍是寬字串指標，
    // 直接轉型傳位址即可）。
    // SAFETY: wav 為 'static、播放期間恆有效；hmod 傳 null（記憶體播放不需模組控制代碼）。
    unsafe {
        PlaySoundW(
            wav.as_ptr() as *const u16,
            std::ptr::null_mut(),
            SND_MEMORY | SND_ASYNC,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// 回傳 (聲道數, 秒數, 峰值 0~1)。
    fn describe(wav: &[u8]) -> (u16, f32, f32) {
        let reader = hound::WavReader::new(Cursor::new(wav)).expect("內嵌的提示音必須是合法的 WAV");
        let spec = reader.spec();
        let secs = reader.duration() as f32 / spec.sample_rate as f32;
        let peak = reader
            .into_samples::<i16>()
            .map(|s| (s.unwrap() as f32 / 32768.0).abs())
            .fold(0.0f32, f32::max);
        (spec.channels, secs, peak)
    }

    /// 提示音要短：筆電的麥克風會把它收進錄音，越短越不容易干擾「有沒有說話」的判斷
    /// （參考 SayIt：一聲 0.15 秒）。音量也不能太大聲或太小聲。
    #[test]
    fn both_cues_are_short_mono_and_moderate_in_volume() {
        for (name, wav) in [("start", START_WAV), ("stop", STOP_WAV)] {
            let (channels, secs, peak) = describe(wav);
            assert_eq!(channels, 1, "{name} 應為單聲道");
            assert!(secs > 0.05 && secs <= 0.2, "{name} 長度 {secs:.2}s 應在 0.05~0.2 秒");
            assert!((0.1..=0.25).contains(&peak), "{name} 峰值 {peak:.2} 應在 0.1~0.25");
        }
    }

    /// 用過零點的次數估計平均頻率（Hz）。提示音是單一純音（可能音高滑動），這樣估就夠準。
    fn average_frequency(wav: &[u8]) -> f32 {
        let reader = hound::WavReader::new(Cursor::new(wav)).unwrap();
        let rate = reader.spec().sample_rate as f32;
        let samples: Vec<i16> = reader.into_samples::<i16>().map(|s| s.unwrap()).collect();
        let secs = samples.len() as f32 / rate;
        let (mut crossings, mut last) = (0u32, 0i16);
        for &x in samples.iter().filter(|&&x| x != 0) {
            if last != 0 && (x > 0) != (last > 0) {
                crossings += 1;
            }
            last = x;
        }
        crossings as f32 / (2.0 * secs)
    }

    /// 2026-11-16 使用者要求：比 SayIt 的 880Hz 更低沉柔和、帶一點科技感（音高滑動）。
    /// 開始音往上滑、結束音往下滑，平均頻率都要低於 800Hz，且開始音比結束音高。
    #[test]
    fn cues_are_lower_than_880hz_and_start_is_higher_than_stop() {
        let (start, stop) = (average_frequency(START_WAV), average_frequency(STOP_WAV));
        assert!((300.0..800.0).contains(&start), "開始音平均 {start:.0}Hz 應在 300~800Hz");
        assert!((250.0..800.0).contains(&stop), "結束音平均 {stop:.0}Hz 應在 250~800Hz");
        assert!(start > stop, "開始音 {start:.0}Hz 應比結束音 {stop:.0}Hz 高");
    }

    /// 開始音要比結束音高：一高一低讓使用者不用看畫面也聽得出是開始還是結束。
    #[test]
    fn start_and_stop_cues_are_different_sounds() {
        assert_ne!(START_WAV, STOP_WAV);
    }
}
