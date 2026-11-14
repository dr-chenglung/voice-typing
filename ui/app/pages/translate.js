// 翻譯頁：翻譯模式開關（切換就立即存檔）與目標語言（按「儲存」才存）。
import { makePage, card, hint, textValue } from "../form-page.js";
import { PRESET_LANGUAGES, languageSelection, resolveTargetLanguage } from "../lib/settings.js";
import { escapeHtml } from "../lib/html.js";
import * as api from "../api.js";

const LANGUAGE_LABELS = {
  English: "英文 English",
  Japanese: "日文 Japanese",
  Korean: "韓文 Korean",
  "Simplified Chinese": "簡體中文 Simplified Chinese",
  "Traditional Chinese": "繁體中文 Traditional Chinese",
  Spanish: "西班牙文 Spanish",
  Vietnamese: "越南文 Vietnamese",
};

export const { load, isDirty } = makePage({
  title: "翻譯",
  description: "開啟翻譯模式後，下一次用熱鍵錄音會直接翻譯成目標語言，而不是一般整理。",
  render: (cfg) => {
    const sel = languageSelection(cfg.target_language);
    const options = PRESET_LANGUAGES.map(
      (lang) => `<option value="${lang}"${sel.select === lang ? " selected" : ""}>${LANGUAGE_LABELS[lang]}</option>`,
    ).join("");
    return (
      card(
        "翻譯模式",
        `<label class="checkbox"><input type="checkbox" id="translate_mode_active"${cfg.translate_mode_active ? " checked" : ""} />
          啟用翻譯模式</label>` + hint("切換立即生效，不需要重啟程式；下一次按熱鍵錄音就會套用。"),
      ) +
      card(
        "目標語言",
        `<label>目標語言
          <select id="target_language_select">${options}
            <option value="__custom__"${sel.select === "__custom__" ? " selected" : ""}>自訂…</option>
          </select>
        </label>
        <label id="target_language_custom_row"${sel.select === "__custom__" ? "" : " hidden"}>
          自訂目標語言（英文語言名稱，例：French）
          <input type="text" id="target_language_custom" value="${escapeHtml(sel.custom)}" placeholder="French" />
        </label>` + hint("翻譯使用自己的提示詞，不受「整理風格」頁的選擇影響。"),
      )
    );
  },
  read: (s) => ({
    target_language: resolveTargetLanguage(
      textValue(s, "target_language_select"),
      textValue(s, "target_language_custom"),
    ),
  }),
  bind: (section, ctx, setStatus) => {
    const select = section.querySelector("#target_language_select");
    const customRow = section.querySelector("#target_language_custom_row");
    select.addEventListener("change", () => {
      customRow.hidden = select.value !== "__custom__";
    });

    // 開關切換就立即存；失敗時還原成原本的狀態，並顯示原因。
    const toggle = section.querySelector("#translate_mode_active");
    toggle.addEventListener("change", async () => {
      const wanted = toggle.checked;
      try {
        await api.saveFields({ translate_mode_active: wanted });
        setStatus(wanted ? "翻譯模式已開啟" : "翻譯模式已關閉");
        ctx.refreshStatus();
      } catch (err) {
        toggle.checked = !wanted;
        setStatus(`切換失敗：${err}`, true);
      }
    });
  },
});
