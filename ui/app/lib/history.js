// 歷史紀錄的純邏輯（不碰 DOM）。後端已用新到舊排序回傳，這裡不重新排序。

/** 依關鍵字篩選：不分大小寫，同時比對輸出文字與翻譯前的原文；查詢為空就原樣回傳。 */
export function filterHistory(entries, query) {
  const needle = String(query ?? "").trim().toLowerCase();
  if (!needle) return entries;
  return entries.filter(
    (e) =>
      String(e.text ?? "").toLowerCase().includes(needle) ||
      String(e.source_text ?? "").toLowerCase().includes(needle),
  );
}

/** 最近一筆（後端是新到舊，所以是第一筆）；沒有紀錄回 null。 */
export function latestEntry(entries) {
  return entries.length > 0 ? entries[0] : null;
}
