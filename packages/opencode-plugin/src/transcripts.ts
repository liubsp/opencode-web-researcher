import { Service } from "@opencode/client/service";
import { readFile, mkdir, writeFile, readdir, stat, unlink } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve, join } from "node:path";

export async function discoverTempDirectory(): Promise<string> {
  const endpoint = await Service.discover();
  if (!endpoint) throw new Error("Cannot discover OpenCode's temporary directory");
  const response = await fetch(new URL("/api/info", endpoint.url), { headers: Service.headers(endpoint), signal: AbortSignal.timeout(10000) });
  if (!response.ok) throw new Error("Cannot read OpenCode server paths");
  const info = await response.json() as { paths?: { tmp?: string } };
  if (!info.paths?.tmp) throw new Error("OpenCode did not report paths.tmp");
  return resolve(info.paths.tmp, "web-research-transcripts");
}

// Share disposable Markdown copies in OpenCode's already-approved temporary directory.
export async function publishTranscripts(value: unknown, directory: string): Promise<unknown> {
  if (!value || typeof value !== "object") return value;
  const output = value as Record<string, unknown>;
  const entries = [output, ...(Array.isArray(output.results) ? output.results : [])];
  for (const entry of entries) {
    if (!entry || typeof entry !== "object") continue;
    const local = (entry as Record<string, unknown>).local_transcript as { markdown?: {path?: string}; error?: string } | undefined;
    if (!local) continue;
    // Internal server paths are not the parent-facing artifacts.
    delete (local as Record<string, unknown>).archive_markdown;
    const source = local.markdown?.path;
    if (!source) continue;
    try {
      const markdown = await readFile(source);
      await mkdir(directory, { recursive: true });
      const name = createHash("sha256").update(source).update(markdown).digest("hex") + ".md";
      const path = join(directory, name);
      await writeFile(path, markdown);
      Object.assign(local, {markdown: {path}, temporary: true});
    } catch (error) {
      Object.assign(local, {markdown: null, error: `Temporary transcript export failed: ${String(error)}`});
    }
  }
  return output;
}

export async function cleanTempTranscripts(directory: string): Promise<void> {
  let files: string[];
  try { files = await readdir(directory); } catch { return; }
  for (const file of files) {
    if (!/^[a-f0-9]{64}\.md$/.test(file)) continue;
    const path = join(directory, file);
    try { if (Date.now() - (await stat(path)).mtimeMs > 30 * 86400000) await unlink(path); } catch { /* Another location may have removed it. */ }
  }
}
