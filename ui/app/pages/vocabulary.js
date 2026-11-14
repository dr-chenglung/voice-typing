// 個人詞彙頁：同時用於語音辨識引導與 AI 整理時修正誤辨的專有名詞。
import { makePage, card, hint, textValue } from "../form-page.js";
import { escapeHtml } from "../lib/html.js";

export const { load, isDirty } = makePage({
  title: "個人詞彙",
  description: "放常用的專有名詞，幫助辨識與整理時寫對。",
  render: (cfg) =>
    card(
      "個人詞彙表",
      `<label>詞彙（逗號或換行分隔）
        <textarea id="vocabulary" rows="8" placeholder="常用專有名詞，逗號或換行分隔，例：Tauri, Rust, cpal, 系統列">${escapeHtml(cfg.vocabulary)}</textarea>
      </label>` + hint("同時用於語音辨識引導（Whisper prompt）與校正時修正音譯/誤辨的專有名詞。"),
    ),
  read: (s) => ({ vocabulary: textValue(s, "vocabulary") }),
});
