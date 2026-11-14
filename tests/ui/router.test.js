import { test } from "node:test";
import assert from "node:assert/strict";
import { parsePage, createNavigator, PAGES } from "../../ui/app/lib/router.js";

test("parsePage 依序取 hash、stored，都無效時回 home", () => {
  assert.equal(parsePage("#history", "stt"), "history");
  assert.equal(parsePage("", "stt"), "stt");
  assert.equal(parsePage("#foo", null), "home");
  assert.equal(parsePage("", "removed-page"), "home");
});

test("有未存修改且使用者取消時，go() 不切換並回 false", async () => {
  const shown = [];
  const nav = createNavigator({ isDirty: () => true, askConfirm: async () => false, show: async (id) => shown.push(id) });
  assert.equal(await nav.go("history"), false);
  assert.deepEqual(shown, []);
});

test("有未存修改且使用者確認時，go() 切換", async () => {
  const shown = [];
  const nav = createNavigator({ isDirty: () => true, askConfirm: async () => true, show: async (id) => shown.push(id) });
  assert.equal(await nav.go("history"), true);
  assert.equal(nav.current(), "history");
});

test("go() 到不存在的頁面時改去 home", async () => {
  const shown = [];
  const nav = createNavigator({ isDirty: () => false, askConfirm: async () => true, show: async (id) => shown.push(id) });
  await nav.go("nope");
  assert.deepEqual(shown, ["home"]);
});

test("go() 到目前所在的頁面時不重複切換，也不詢問", async () => {
  const shown = [];
  let asked = 0;
  const nav = createNavigator({ isDirty: () => true, askConfirm: async () => { asked++; return true; }, show: async (id) => shown.push(id) });
  await nav.go("stt");
  asked = 0;
  assert.equal(await nav.go("stt"), true);
  assert.deepEqual(shown, ["stt"]);
  assert.equal(asked, 0);
});

test("PAGES 的 id 順序固定", () => {
  assert.deepEqual(PAGES.map((p) => p.id),
    ["home", "stt", "llm", "style", "translate", "vocabulary", "history", "general", "about"]);
});

test("general 與 about 屬於 footer 群組，其餘沒有群組", () => {
  assert.deepEqual(PAGES.filter((p) => p.group === "footer").map((p) => p.id), ["general", "about"]);
});
