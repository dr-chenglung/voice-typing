// 語音辨識頁：STT 的 API 金鑰、網址、模型（欄位與提示文字照搬舊設定視窗）。
import { makePage, card, keyInput, textInput, textValue } from "../form-page.js";

export const { load, isDirty } = makePage({
  title: "語音辨識",
  description: "設定語音轉文字要使用的服務。",
  render: (cfg) =>
    card(
      "語音轉文字 (STT)",
      keyInput("stt_api_key", "STT API Key", cfg.stt_api_key, "例如 gsk_…；本機 API 隨便填一個字串即可") +
        textInput("stt_api_url", "STT API URL", cfg.stt_api_url, "https://api.groq.com/openai/v1/audio/transcriptions") +
        textInput("stt_model", "STT 模型", cfg.stt_model),
    ),
  read: (s) => ({
    stt_api_key: textValue(s, "stt_api_key"),
    stt_api_url: textValue(s, "stt_api_url"),
    stt_model: textValue(s, "stt_model"),
  }),
});
