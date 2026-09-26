#!/usr/bin/env node
import { readFile, writeFile, mkdir, access, realpath } from "node:fs/promises";
import { resolve, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import { homedir } from "node:os";
import { applyEdits, modify, parse, type ParseError } from "jsonc-parser";

export function mergePlugin(source: string, packagePath: string, binary: string): string {
  const errors: ParseError[] = [];
  const config = parse(source, errors, { allowTrailingComma: true, disallowComments: false }) as Record<string, unknown>;
  if (errors.length || !config || Array.isArray(config) || typeof config !== "object") throw new Error("Invalid OpenCode JSONC; not modifying it");
  const existing = config.plugins ?? [];
  if (!Array.isArray(existing)) throw new Error("OpenCode plugins must be an array");
  const legacyPath = packagePath.replace(/\/opencode-web-researcher\/?$/, "/web-research-opencode/");
  const plugins = existing.filter(entry => {
    const name = typeof entry === "object" && entry ? entry.package : entry;
    return typeof name !== "string" || ![packagePath, legacyPath].some(path => name.replace(/\/$/, "") === path.replace(/\/$/, ""));
  });
  plugins.push({ package: packagePath, options: { binary } });
  return applyEdits(source, modify(source, ["plugins"], plugins, { formattingOptions: { insertSpaces: true, tabSize: 2 } }));
}

async function exists(path: string): Promise<boolean> { try { await access(path); return true; } catch { return false; } }

export async function install(project: string, binary: string, global = false, previousAgent?: string): Promise<void> {
  if (global) await mkdir(project, { recursive: true });
  const root = await realpath(project);
  const executable = await realpath(binary);
  const packageDir = fileURLToPath(new URL("../", import.meta.url));
  const agent = await readFile(resolve(packageDir, "agents/web-researcher.md"), "utf8");
  const agentPath = resolve(root, global ? "agents/web-researcher.md" : ".opencode/agents/web-researcher.md");
  if (await exists(agentPath) && ![agent, previousAgent].includes(await readFile(agentPath, "utf8"))) {
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
  console.log(`Installed web-researcher ${global ? "globally" : "for project"}: ${root}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const { values } = parseArgs({ options: { project: { type: "string" }, binary: { type: "string" }, global: { type: "boolean" }, "previous-agent": { type: "string" } } });
  if (!values.binary || Boolean(values.project) === Boolean(values.global)) throw new Error("Usage: web-research-setup (--global | --project <directory>) --binary <server executable>");
  const root = values.global ? resolve(process.env.XDG_CONFIG_HOME || resolve(homedir(), ".config"), "opencode") : values.project!;
  await install(root, values.binary, values.global, values["previous-agent"] ? await readFile(values["previous-agent"], "utf8") : undefined);
}
