// AI 整理風格頁：用單選決定「不使用 AI 整理／基本型／積極型／自訂」，點選立即生效。
// 基本型、積極型的提示詞唯讀顯示（讓使用者知道它怎麼寫的）；自訂型由使用者自己寫整份提示詞，
// 文字存在 Rust 端的 custom_prompt.txt（不進 config.toml）。
import * as api from "../api.js";
import { STYLE_CHOICES, styleChoiceFromConfig, configPatchForStyle, planCustomSelection } from "../lib/settings.js";
import { escapeHtml as esc } from "../lib/html.js";

const CUSTOM_WARNING =
  "這份提示詞完全由你負責。刪掉「不回答逐字稿」「不翻譯」這類防護規則，模型可能把逐字稿裡的問題當成要回答的問題。" +
  "個人詞彙表有填的話，會自動附加在提示詞後面。";

let section = null;
let ctx = null;
let prompts = { basic: "", active: "", custom: "" };
let savedCustom = ""; // 已經存檔的自訂文字
let customText = ""; // 文字框目前的內容；重畫時要保留，所以不能只放在 DOM 裡
const expanded = new Set(); // 目前展開提示詞的卡片

function setStatus(text, isError = false) {
  const el = section.querySelector(".status-text");
  el.textContent = text;
  el.classList.toggle("error", isError);
}

function cardHtml(choice, isActive) {
  const id = esc(choice.id);
  const open = expanded.has(choice.id);
  let toggle = "";
  let panel = "";
  if (choice.id === "basic" || choice.id === "active") {
    toggle = `<button type="button" class="link" data-view="${id}">${open ? "▾" : "▸"} 檢視提示詞</button>`;
    panel = `<div class="prompt-panel" data-panel="${id}"${open ? "" : " hidden"}>
      <textarea class="prompt-text" data-prompt="${id}" readonly rows="12">${esc(prompts[choice.id])}</textarea>
    </div>`;
  } else if (choice.id === "custom") {
    toggle = `<button type="button" class="link" data-view="custom">${open ? "▾" : "▸"} 編輯提示詞</button>`;
    panel = `<div class="prompt-panel" data-panel="custom"${open ? "" : " hidden"}>
      <p class="hint warning-text">${esc(CUSTOM_WARNING)}</p>
      <textarea class="prompt-text" data-prompt="custom" rows="12" spellcheck="false" placeholder="在這裡寫你自己的整份提示詞；也可以先按下面的按鈕複製內建提示詞再修改。">${esc(customText)}</textarea>
      <div class="actions">
        <button type="button" class="secondary small" data-copy="basic">從基本型複製</button>
        <button type="button" class="secondary small" data-copy="active">從積極型複製</button>
        <button type="button" class="small" data-save-custom>儲存</button>
      </div>
    </div>`;
  }
  return `<div class="style-card${isActive ? " active" : ""}">
    <label class="style-head">
      <input type="radio" name="style_choice" value="${id}"${isActive ? " checked" : ""} />
      <span class="style-text"><span class="style-name">${esc(choice.name)}</span><span class="style-desc">${esc(choice.description)}</span></span>
    </label>
    ${toggle}${panel}
  </div>`;
}

async function draw(note = "", isError = false) {
  const [cfg, loaded] = await Promise.all([api.getConfig(), api.getStylePrompts()]);
  prompts = loaded;
  const current = styleChoiceFromConfig(cfg);
  section.innerHTML = `
    <h1 class="page-title">AI 整理風格</h1>
    <p class="page-desc">決定辨識出的文字要不要交給語言模型整理，以及怎麼整理。選項點一下就生效。</p>
    <div class="style-list">${STYLE_CHOICES.map((c) => cardHtml(c, c.id === current)).join("")}</div>
    <p class="hint">需要在「AI 整理 API」頁填好語言模型的連線才會生效。翻譯模式使用自己的提示詞，只在選「積極型」時一併套用智慧排版。</p>
    <span class="status-text" role="status"></span>`;
  bind();
  if (note) setStatus(note, isError);
}

function bind() {
  for (const input of section.querySelectorAll("input[name=style_choice]")) {
    input.addEventListener("change", () => choose(input.value));
  }
  for (const btn of section.querySelectorAll("button[data-view]")) {
    btn.addEventListener("click", () => toggleView(btn));
  }
  const textarea = section.querySelector("textarea[data-prompt=custom]");
  textarea.addEventListener("input", () => {
    customText = textarea.value;
  });
  for (const btn of section.querySelectorAll("button[data-copy]")) {
    btn.addEventListener("click", async () => {
      // 文字框裡有還沒存的內容時，複製會把它蓋掉：先確認，免得誤按就弄丟一長段文字。
      if (customText.trim() && customText !== savedCustom) {
        if (!(await ctx.confirm("文字框裡有還沒儲存的內容，複製會覆蓋它，確定要覆蓋嗎？"))) return;
      }
      customText = prompts[btn.dataset.copy];
      textarea.value = customText;
    });
  }
  section.querySelector("button[data-save-custom]").addEventListener("click", saveCustom);
}

// 展開／收合只動這一張卡片，不重畫整頁，才不會弄丟文字框裡還沒存的內容。
function toggleView(btn) {
  const id = btn.dataset.view;
  const panel = section.querySelector(`div[data-panel=${id}]`);
  const open = panel.hidden; // 目前收合 → 要展開
  panel.hidden = !open;
  if (open) expanded.add(id);
  else expanded.delete(id);
  btn.textContent = `${open ? "▾" : "▸"} ${id === "custom" ? "編輯提示詞" : "檢視提示詞"}`;
}

async function choose(id) {
  setStatus("");
  let saveFirst = false;
  if (id === "custom") {
    const plan = planCustomSelection(customText, savedCustom);
    if (plan.error) {
      // 文字框空白：直接顯示錯誤並展開編輯區，不存任何東西（避免蓋掉已存的提示詞）。
      expanded.add("custom");
      await draw(plan.error, true);
      return;
    }
    saveFirst = plan.saveFirst;
  }
  try {
    // 選自訂時，文字框裡還沒存的內容要先存起來，否則選了卻讀不到。
    if (saveFirst) {
      await api.saveCustomPrompt(customText);
      savedCustom = customText;
    }
    await api.saveFields(configPatchForStyle(id));
  } catch (err) {
    // 失敗（例如自訂文字還是空的）：展開編輯區讓使用者看到，並重畫以還原單選。
    if (id === "custom") expanded.add("custom");
    await draw(String(err), true);
    return;
  }
  if (id === "custom") expanded.add("custom");
  await draw("已切換");
  ctx.refreshStatus();
}

async function saveCustom() {
  try {
    await api.saveCustomPrompt(customText);
    savedCustom = customText;
    setStatus("已儲存");
  } catch (err) {
    setStatus(`儲存失敗：${err}`, true);
  }
}

export async function load(sec, context) {
  section = sec;
  ctx = context;
  const [cfg, loaded] = await Promise.all([api.getConfig(), api.getStylePrompts()]);
  savedCustom = loaded.custom;
  customText = loaded.custom;
  expanded.clear();
  if (styleChoiceFromConfig(cfg) === "custom") expanded.add("custom");
  await draw();
}

/** 自訂文字框有還沒存的內容時，離開頁面前要先確認。 */
export function isDirty() {
  return customText !== savedCustom;
}
