import { test } from "node:test";
import assert from "node:assert/strict";
import { escapeHtml } from "../../ui/app/lib/html.js";

test("escapeHtml 跳脫 & < > \" '", () => {
  assert.equal(escapeHtml(`a<b & "c" 'd'>`), "a&lt;b &amp; &quot;c&quot; &#39;d&#39;&gt;");
});

test("escapeHtml 不動一般中文", () => assert.equal(escapeHtml("今天天氣很好"), "今天天氣很好"));

test("escapeHtml 把非字串當成空字串", () => {
  assert.equal(escapeHtml(undefined), "");
  assert.equal(escapeHtml(null), "");
});
