// 一般設定頁：熱鍵，以及設定檔位置（顯示路徑、開啟資料夾方便備份）。
import { makePage, card, hint, textValue } from "../form-page.js";
import * as api from "../api.js";

export const { load, isDirty } = makePage({
  title: "一般設定",
  description: "熱鍵與設定檔位置。",
  render: (cfg) =>
    card(
      "熱鍵設定",
      `<label>熱鍵
        <select id="hotkey">
          <option value="right_alt"${cfg.hotkey === "right_alt" ? " selected" : ""}>右 Alt（預設）</option>
          <option value="right_ctrl"${cfg.hotkey === "right_ctrl" ? " selected" : ""}>右 Ctrl</option>
        </select>
      </label>` + hint("變更熱鍵後需要重新啟動程式才會生效。"),
    ) +
    card(
      "設定檔位置",
      `<p class="hint" id="config_path">讀取中…</p>
       <button type="button" class="secondary" id="open_config_folder">開啟資料夾</button>`,
    ),
  read: (s) => ({ hotkey: textValue(s, "hotkey") }),
  savedNote: (before, after) =>
    before.hotkey !== after.hotkey ? "（熱鍵已變更，需重啟程式才生效）" : "",
  bind: (section, _ctx, setStatus) => {
    const pathEl = section.querySelector("#config_path");
    api
      .invoke("get_config_path")
      .then((p) => (pathEl.textContent = p))
      .catch((err) => (pathEl.textContent = `無法取得路徑：${err}`));
    section.querySelector("#open_config_folder").addEventListener("click", () => {
      api.invoke("open_config_folder").catch((err) => setStatus(`無法開啟資料夾：${err}`, true));
    });
  },
});
