import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import plugin from "../src/index.ts";

test("registered plugin ID matches the package name", async () => {
  const manifest = JSON.parse(await readFile(new URL("../package.json", import.meta.url), "utf8"));
  assert.equal(plugin.id, manifest.name);
});
