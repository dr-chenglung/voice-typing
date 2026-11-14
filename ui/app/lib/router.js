// 頁面清單與頁面切換邏輯（純函式，不碰 DOM，方便用 node --test 測試）。
// 順序必須與 Rust 端 tray.rs 的 PAGE_IDS 相同（tray.rs 有測試釘住）。

export const PAGES = [
  { id: "home", label: "主頁" },
  { id: "stt", label: "語音辨識" },
  { id: "llm", label: "AI 整理" },
  { id: "style", label: "整理風格" },
  { id: "translate", label: "翻譯" },
  { id: "vocabulary", label: "個人詞彙" },
  { id: "history", label: "歷史紀錄" },
  { id: "general", label: "一般設定", group: "footer" },
  { id: "about", label: "關於", group: "footer" },
];

const PAGE_IDS = new Set(PAGES.map((p) => p.id));

/** 決定開啟時要顯示哪一頁：網址 hash 優先，其次是上次記住的頁面，都無效就回主頁。 */
export function parsePage(hash, stored) {
  const fromHash = String(hash ?? "").replace(/^#/, "");
  if (PAGE_IDS.has(fromHash)) return fromHash;
  if (stored && PAGE_IDS.has(stored)) return stored;
  return "home";
}

/**
 * 所有切頁都經過 go()：側邊欄點擊、托盤選單、主頁的「更換」連結都一樣，
 * 所以「有未存修改時要先確認」只需要在這裡做一次。
 * 一開始還沒有任何頁面（current 為空字串），第一次 go() 一定會呼叫 show()。
 */
export function createNavigator({ isDirty, askConfirm, show }) {
  let current = "";
  return {
    current: () => current,
    async go(id) {
      const target = PAGE_IDS.has(id) ? id : "home";
      if (target === current) return true;
      if (isDirty() && !(await askConfirm())) return false;
      await show(target);
      current = target;
      return true;
    },
  };
}
