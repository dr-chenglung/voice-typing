// 歷史紀錄頁（空殼，後續 task 會補上內容）。
export async function load(section) {
  section.innerHTML = '<h1 class="page-title">歷史紀錄</h1>';
}

export function isDirty() {
  return false;
}
