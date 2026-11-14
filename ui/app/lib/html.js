// 把文字放進 innerHTML 之前一律先經過這裡，避免歷史紀錄等內容被當成 HTML 解析。

const ESCAPES = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };

export function escapeHtml(text) {
  return String(text ?? "").replace(/[&<>"']/g, (ch) => ESCAPES[ch]);
}
