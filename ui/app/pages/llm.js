// AI 整理頁：是否啟用 LLM 輕度校正，以及 LLM 的 API 金鑰、網址、模型。
// 智慧排版開關移到「整理風格」頁；個人詞彙獨立成一頁。
import { makePage, card, checkbox, hint, keyInput, textInput, textValue, checkedValue } from "../form-page.js";

export const { load, isDirty } = makePage({
  title: "AI 整理",
  description: "把辨識出的文字交給語言模型整理。關閉時直接輸出辨識原文；翻譯模式不受這個開關影響。",
  render: (cfg) =>
    card("", checkbox("enable_correction", "啟用 AI 整理（LLM 輕度校正）", cfg.enable_correction)) +
    card(
      "文字校正 (LLM)",
      keyInput("llm_api_key", "LLM API Key", cfg.llm_api_key, "例如 gsk_…；本機 API 隨便填一個字串即可") +
        textInput("llm_api_url", "LLM API URL", cfg.llm_api_url, "https://api.groq.com/openai/v1/chat/completions") +
        textInput("llm_model", "校正用 LLM 模型", cfg.llm_model) +
        hint("這組連線也會用在翻譯模式。"),
    ),
  read: (s) => ({
    enable_correction: checkedValue(s, "enable_correction"),
    llm_api_key: textValue(s, "llm_api_key"),
    llm_api_url: textValue(s, "llm_api_url"),
    llm_model: textValue(s, "llm_model"),
  }),
});
