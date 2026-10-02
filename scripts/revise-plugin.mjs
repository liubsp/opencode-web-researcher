// Immutable import paths let a configuration reload escape cached plugin dependencies.
import { createHash } from "node:crypto";
import { readdir, readFile, mkdir, copyFile, writeFile } from "node:fs/promises";
import { resolve, join, dirname, basename, relative } from "node:path";

const root = process.argv[2] && resolve(process.argv[2]);
if (!root) throw new Error("Usage: revise-plugin.mjs <installed-package-directory>");
const manifest = JSON.parse(await readFile(join(root, "package.json"), "utf8"));
if (manifest.name !== "opencode-web-researcher") throw new Error("Unexpected plugin package");
const directory = join(root, "dist");
const files = (await readdir(directory, { withFileTypes: true })).filter(entry => entry.isFile() && entry.name.endsWith(".js")).map(entry => entry.name).sort();
if (!files.includes("index.js")) throw new Error("Missing plugin build");
const hash = createHash("sha256");
for (const name of files) { hash.update(name); hash.update(await readFile(join(directory, name))); }
const revision = hash.digest("hex").slice(0, 16);
// Keep registered packages outside npm's replacement/pruning boundary. Imports
// still discover the installed dependencies through runtime/node_modules.
const target = basename(dirname(root)) === "node_modules"
  ? resolve(root, "../../plugin-revisions", manifest.name, revision)
  : join(directory, "updates", revision);
await mkdir(target, { recursive: true });
for (const name of files) await copyFile(join(directory, name), join(target, name));
await writeFile(join(target, "package.json"), JSON.stringify({ name: manifest.name, version: manifest.version, private: true, type: "module", main: "./index.js", exports: { ".": "./index.js", "./server": "./index.js" } }, null, 2) + "\n");
const location = relative(root, target).replaceAll("\\", "/");
await writeFile(join(root, "runtime-revision.json"), JSON.stringify({ revision, directory: location }) + "\n");
await writeFile(join(root, "server.js"), `export { default } from ${JSON.stringify(`${location.startsWith(".") ? "" : "./"}${location}/index.js`)};\n`);
console.log(`Plugin module revision: ${revision}`);
