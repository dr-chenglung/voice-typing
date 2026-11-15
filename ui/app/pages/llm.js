// AI 整理 API 頁：整理文字與翻譯共用的語言模型連線（金鑰、網址、模型）。
// 「是否使用 AI 整理、怎麼整理」改到「AI 整理風格」頁選擇，這頁不再有啟用勾選框。
import { makePage, card, hint, keyInput, textInput, textValue } from "../form-page.js";

export const { load, isDirty } = makePage({
  title: "AI 整理 API",
  description: "設定整理文字與翻譯要使用的語言模型連線。是否使用 AI 整理、怎麼整理，請到「AI 整理風格」頁選擇。",
  render: (cfg) =>
    card(
      "語言模型 (LLM)",
      keyInput("llm_api_key", "LLM API Key", cfg.llm_api_key, "例如 gsk_…；本機 API 隨便填一個字串即可") +
        textInput("llm_api_url", "LLM API URL", cfg.llm_api_url, "https://api.groq.com/openai/v1/chat/completions") +
        textInput("llm_model", "校正用 LLM 模型", cfg.llm_model) +
        hint("這組連線也會用在翻譯模式。"),
    ),
  read: (s) => ({
    llm_api_key: textValue(s, "llm_api_key"),
    llm_api_url: textValue(s, "llm_api_url"),
    llm_model: textValue(s, "llm_model"),
  }),
});
