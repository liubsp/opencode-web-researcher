import { test } from "node:test";
import assert from "node:assert/strict";
import { parse } from "jsonc-parser";
import { install, mergePlugin } from "../src/install.ts";
import { mkdtemp, readFile, writeFile, rm, access } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

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

test("package rename replaces the legacy registration without duplicating tools", () => {
  const current = "file:///data/app/runtime/node_modules/opencode-web-researcher/";
  const legacy = "file:///data/app/runtime/node_modules/web-research-opencode";
  for (const entry of [legacy, {package: legacy, options: {binary: "old"}}]) {
    const source = JSON.stringify({plugins: ["other-plugin", entry]});
    const updated = mergePlugin(source, current, "server");
    assert.deepEqual(parse(updated).plugins, ["other-plugin", {package: current, options: {binary: "server"}}]);
    assert.equal(mergePlugin(updated, current, "server"), updated);
  }
});

test("global setup uses the global agents directory and preserves customized instructions", async () => {
  const root = await mkdtemp(join(tmpdir(), "research-global-"));
  try {
    await writeFile(join(root, "opencode.jsonc"), '{ // keep me\n "shell":"custom"\n}');
    await install(root, process.execPath, true);
    const agent = join(root, "agents/web-researcher.md");
    assert.match(await readFile(agent, "utf8"), /mode: subagent/);
    await assert.rejects(access(join(root, ".opencode")));
    await install(root, process.execPath, true);
    const config = await readFile(join(root, "opencode.jsonc"), "utf8");
    assert.match(config, /keep me/);
    assert.equal(parse(config).plugins.length, 1);
    await writeFile(agent, "custom instructions");
    await assert.rejects(install(root, process.execPath, true), /Existing agent differs/);
    assert.equal(await readFile(agent, "utf8"), "custom instructions");
  } finally { await rm(root, {recursive:true, force:true}); }
});
