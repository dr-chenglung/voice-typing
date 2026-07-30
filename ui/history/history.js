// 歷史紀錄視窗：讀取/清除 history.json（經由 Rust 端 get_history/clear_history command）。
// 後端已用新到舊排序回傳，這裡不重新排序。

const { core, event } = window.__TAURI__;

const listEl = document.getElementById("list");
const emptyEl = document.getElementById("empty");
const clearBtn = document.getElementById("clear");

function formatTimestamp(unixSeconds) {
  return new Date(unixSeconds * 1000).toLocaleString();
}

function render(entries) {
  listEl.innerHTML = "";
  emptyEl.classList.toggle("hidden", entries.length > 0);
  for (const entry of entries) {
    const li = document.createElement("li");

    const ts = document.createElement("span");
    ts.className = "timestamp";
    ts.textContent = formatTimestamp(entry.timestamp);
    if (entry.translated) {
      const badge = document.createElement("span");
      badge.className = "badge";
      badge.textContent = `翻譯 → ${entry.target_language}`;
      ts.appendChild(document.createTextNode(" "));
      ts.appendChild(badge);
    }

    const text = document.createElement("span");
    text.className = "text";
    text.textContent = entry.text;

    li.appendChild(ts);
    li.appendChild(text);

    if (entry.translated && entry.source_text) {
      const source = document.createElement("span");
      source.className = "text source hidden";
      source.textContent = entry.source_text;

      const toggle = document.createElement("button");
      toggle.className = "toggle-source";
      toggle.textContent = "顯示原文";
      toggle.addEventListener("click", () => {
        const hidden = source.classList.toggle("hidden");
        toggle.textContent = hidden ? "顯示原文" : "隱藏原文";
      });

      li.appendChild(toggle);
      li.appendChild(source);
    }

    listEl.appendChild(li);
  }
}

async function load() {
  const entries = await core.invoke("get_history");
  render(entries);
}

clearBtn.addEventListener("click", async () => {
  if (!confirm("確定要清除全部歷史紀錄嗎？此動作無法復原。")) {
    return;
  }
  await core.invoke("clear_history");
  await load();
});

event.listen("history-cleared", () => load());

load();
