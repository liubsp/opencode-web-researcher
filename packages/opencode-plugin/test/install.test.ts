import { test } from "node:test";
import assert from "node:assert/strict";
import { parse } from "jsonc-parser";
import { mergePlugin } from "../src/install.ts";

test("project installation preserves comments/settings and does not duplicate registrations", () => {
  const source = '{\n // my config\n "model":"custom", "plugins":["other-plugin"],\n}';
  const once = mergePlugin(source, "file:///plugin", "C:/tool.exe");
  const twice = mergePlugin(once, "file:///plugin", "C:/updated.exe");
  assert.match(twice, /my config/);
  assert.equal(parse(twice).model, "custom");
  assert.deepEqual(parse(twice).plugins, ["other-plugin", { package: "file:///plugin", options: { binary: "C:/updated.exe" } }]);
  assert.throws(() => mergePlugin('{ broken', 'plugin', 'binary'), /Invalid/);
  assert.throws(() => mergePlugin('{"plugins":{}}', 'plugin', 'binary'), /array/);
});
