#!/usr/bin/env node
import { readFile, writeFile, mkdir, access, realpath } from "node:fs/promises";
import { resolve, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import { applyEdits, modify, parse, type ParseError } from "jsonc-parser";

export function mergePlugin(source: string, packagePath: string, binary: string): string {
  const errors: ParseError[] = [];
  const config = parse(source, errors, { allowTrailingComma: true, disallowComments: false }) as Record<string, unknown>;
  if (errors.length || !config || Array.isArray(config) || typeof config !== "object") throw new Error("Invalid OpenCode JSONC; not modifying it");
  const existing = config.plugins ?? [];
  if (!Array.isArray(existing)) throw new Error("OpenCode plugins must be an array");
  const plugins = existing.filter(entry => !(typeof entry === "object" && entry?.package === packagePath) && entry !== packagePath);
  plugins.push({ package: packagePath, options: { binary } });
  return applyEdits(source, modify(source, ["plugins"], plugins, { formattingOptions: { insertSpaces: true, tabSize: 2 } }));
}

async function exists(path: string): Promise<boolean> { try { await access(path); return true; } catch { return false; } }

export async function install(project: string, binary: string): Promise<void> {
  const root = await realpath(project);
  const executable = await realpath(binary);
  const packageDir = fileURLToPath(new URL("../", import.meta.url));
  const agent = await readFile(resolve(packageDir, "agents/web-researcher.md"), "utf8");
  const agentPath = resolve(root, ".opencode/agents/web-researcher.md");
  if (await exists(agentPath) && await readFile(agentPath, "utf8") !== agent) {
    throw new Error(`Existing agent differs: ${agentPath}. Review it before installing.`);
  }
  const jsonc = resolve(root, "opencode.jsonc");
  const json = resolve(root, "opencode.json");
  if (await exists(jsonc) && await exists(json)) throw new Error("Both opencode.json and opencode.jsonc exist; choose one before installing");
  const configPath = await exists(jsonc) ? jsonc : await exists(json) ? json : jsonc;
  const old = await exists(configPath) ? await readFile(configPath, "utf8") : '{\n  "$schema": "https://opencode.ai/config.json"\n}\n';
  const updated = mergePlugin(old, pathToFileURL(packageDir).href, executable);
  await mkdir(dirname(agentPath), { recursive: true });
  await writeFile(agentPath, agent);
  await writeFile(configPath, updated);
  console.log(`Installed web-researcher agent for ${root}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const { values } = parseArgs({ options: { project: { type: "string" }, binary: { type: "string" } } });
  if (!values.project || !values.binary) throw new Error("Usage: web-research-setup --project <directory> --binary <web-research executable>");
  await install(values.project, values.binary);
}
