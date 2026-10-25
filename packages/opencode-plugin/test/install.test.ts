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

test("an application installed elsewhere retains its explicit research data home", () => {
  const updated = mergePlugin("{}", "file:///custom-app/plugin", "/custom-app/bin/server", "/separate-data-home");
  assert.deepEqual(parse(updated).plugins[0].options, { binary: "/custom-app/bin/server", home: "/separate-data-home" });
});

test("immutable plugin revisions replace the same installation without duplicating tools", () => {
  const root = "file:///app/node_modules/opencode-web-researcher";
  const legacy = "file:///app/node_modules/web-research-opencode/";
  const previous = `${root}/dist/updates/0123456789abcdef`;
  const next = "file:///app/plugin-revisions/opencode-web-researcher/fedcba9876543210";
  const source = JSON.stringify({ plugins: [root, legacy, { package: previous, options: { binary: "old" } }, "unrelated"] });
  const updated = mergePlugin(source, next, "new", "/data");
  assert.deepEqual(parse(updated).plugins, ["unrelated", { package: next, options: { binary: "new", home: "/data" } }]);
  assert.deepEqual(parse(mergePlugin(updated, next, "new", "/data")).plugins, parse(updated).plugins);
});

test("global setup uses the global agents directory and preserves customized instructions", async () => {
  const root = await mkdtemp(join(tmpdir(), "research-global-"));
  try {
    await writeFile(join(root, "opencode.jsonc"), '{ // keep me\n "shell":"custom"\n}');
    await install(root, process.execPath, true);
    const agent = join(root, "agents/web-researcher.md");
    const installed = await readFile(agent, "utf8");
    assert.match(installed, /mode: subagent/);
    const description = installed.split("\n").find(line => line.startsWith("description:"))!;
    assert.match(description, /message the SAME researcher session/);
    assert.match(description, /wait for its confirmation/);
    assert.match(description, /Interrupting or terminating.*does NOT cancel service research/);
    assert.match(installed, /Call `research_cancel` for every outstanding request belonging to this task/);
    assert.match(installed, /If it is `cancel_requested`, use `research_wait`\/`research_get`/);
    assert.match(installed, /report the unresolved state and request ID for recovery/);
    const outgoing = installed.split("## Writing to ChatGPT")[1].split("## Research as a conversation")[0];
    assert.match(outgoing, /Keep delegation and agent orchestration out of messages to ChatGPT/);
    assert.match(outgoing, /Preserve truthful attribution of supplied observations and logs/);
    assert.match(outgoing, /Verbatim quotations and relevant discussion of product users/);
    await assert.rejects(access(join(root, ".opencode")));
    await install(root, process.execPath, true);
    const config = await readFile(join(root, "opencode.jsonc"), "utf8");
    assert.match(config, /keep me/);
    assert.equal(parse(config).plugins.length, 1);
    await writeFile(agent, "prior bundled instructions");
    await install(root, process.execPath, true, "prior bundled instructions");
    assert.match(await readFile(agent, "utf8"), /mode: subagent/);
    await writeFile(agent, "custom instructions");
    await assert.rejects(install(root, process.execPath, true, "prior bundled instructions"), /Existing agent differs/);
    assert.equal(await readFile(agent, "utf8"), "custom instructions");
  } finally { await rm(root, {recursive:true, force:true}); }
});
