import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, rm, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import { execFile } from "node:child_process";
import { promisify } from "node:util";

test("configuration reload revisions do not reuse cached plugin dependencies", async () => {
  const runtime = await mkdtemp(join(tmpdir(), "research-revision-"));
  const root = join(runtime, "node_modules", "opencode-web-researcher");
  try {
    await mkdir(join(root, "dist"), { recursive: true });
    await writeFile(join(root, "package.json"), JSON.stringify({ name: "opencode-web-researcher", type: "module" }));
    await writeFile(join(root, "dist", "index.js"), 'export { default } from "./value.js";\n');
    await writeFile(join(root, "dist", "value.js"), 'export default "old";\n');
    const revise = () => promisify(execFile)(process.execPath, [fileURLToPath(new URL("../../../scripts/revise-plugin.mjs", import.meta.url)), root]);
    await revise();
    const entry = async () => {
      const { directory } = JSON.parse(await readFile(join(root, "runtime-revision.json"), "utf8"));
      const target = join(root, directory);
      const manifest = JSON.parse(await readFile(join(target, "package.json"), "utf8"));
      return pathToFileURL(join(target, manifest.exports["./server"])).href;
    };
    const previousEntry = await entry();
    const previous = await import(previousEntry);
    assert.equal(previous.default, "old");
    await writeFile(join(root, "dist", "value.js"), 'export default "new";\n');
    await revise();
    const nextEntry = await entry();
    assert.notEqual(nextEntry, previousEntry);
    assert.equal((await import(nextEntry)).default, "new");
    await revise();
    assert.equal(await entry(), nextEntry);
    assert.equal(previous.default, "old");
    // npm can replace the entire managed package without deleting registered revisions.
    await rm(root, { recursive: true, force: true });
    assert.match(await readFile(fileURLToPath(previousEntry), "utf8"), /value.js/);
    assert.match(await readFile(fileURLToPath(nextEntry), "utf8"), /value.js/);
  } finally { await rm(runtime, { recursive: true, force: true }); }
});
