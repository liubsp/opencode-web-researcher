// Exercise real CLI/bootstrap lifecycle with isolated state; never launch Chrome.
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { mkdtemp, mkdir, copyFile, readFile, writeFile, rm, realpath } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve, join, basename } from "node:path";
import { createServer } from "node:http";

const exec = promisify(execFile);
const binary = resolve("target/release", process.platform === "win32" ? "opencode-web-researcher.exe" : "opencode-web-researcher");
const home = await mkdtemp(join(tmpdir(), "research-runtime-"));
const preferred = join(home, "application", basename(binary));
const env = { ...process.env, WEB_RESEARCH_HOME: home };
const cli = async (file, op) => {
  let timer;
  try {
    // Bound this synthetic fixture even if descendants keep capture pipes open
    // after the launcher exits; execFile's own timeout cannot close those pipes.
    return await Promise.race([
      exec(file, [op], { env, timeout: 120_000, windowsHide: true }),
      new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`Synthetic ${op} did not release captured pipes`)), 30_000); }),
    ]);
  } finally { clearTimeout(timer); }
};
const descriptor = async () => JSON.parse(await readFile(join(home, "service.json"), "utf8"));
const rpc = async (service, input) => {
  const response = await fetch(`http://127.0.0.1:${service.port}/v1/rpc`, {
    method: "POST", headers: { authorization: `Bearer ${service.token}`, "content-type": "application/json" },
    body: JSON.stringify(input), signal: AbortSignal.timeout(10_000),
  });
  const value = await response.json();
  assert.ok(response.ok, value.error);
  return value;
};
const waitForStop = async service => {
  for (let i = 0; i < 100; i++) {
    let registered;
    try { registered = (await descriptor()).instance === service.instance; }
    catch (error) { if (error.code !== "ENOENT") throw error; registered = false; }
    let responding;
    try { responding = (await rpc(service, { op: "health" })).instance === service.instance; }
    catch { responding = false; }
    if (!registered && !responding) return;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  throw new Error("Isolated daemon did not stop");
};
let running;
let previous;
try {
  await mkdir(join(home, "application"));
  await copyFile(binary, preferred);
  await writeFile(join(home, "config.json"), JSON.stringify({ words_per_minute: 1, fixed_pause_seconds: 3600, pause_jitter_seconds: 0, chrome_auto_close: false }));
  running = JSON.parse((await cli(binary, "connect")).stdout);
  const request = { project: "synthetic", session: "synthetic", key: "runtime-test", prompt: "Synthetic work - must never reach ChatGPT", deep_research: false };
  const job = await rpc(running, { op: "submit", request });
  await rpc(running, { op: "shutdown" });
  await waitForStop(running);
  running = undefined;

  // A protocol-compatible earlier build has no build ID. Use a synthetic service
  // for that version boundary; replacement/bootstrap below use the actual binary.
  previous = createServer(async (req, res) => {
    let body = "";
    for await (const chunk of req) body += chunk;
    const input = JSON.parse(body);
    res.setHeader("content-type", "application/json");
    res.end(JSON.stringify(input.op === "health" ? { instance: "previous", protocol: 1 } : { stopping: true }));
    if (input.op === "shutdown") previous.close();
  });
  await new Promise(resolve => previous.listen(0, "127.0.0.1", resolve));
  await writeFile(join(home, "service.json"), JSON.stringify({ protocol: 1, port: previous.address().port, token: "synthetic", instance: "previous", pid: 0 }));
  await cli(preferred, "activate");
  running = await descriptor();
  const health = await rpc(running, { op: "health" });
  assert.match(health.build_id, /^[a-f0-9]{16}$/);
  assert.equal((await rpc(running, { op: "submit", request })).id, job.id);
  const retained = await rpc(running, { op: "get", id: job.id, project: request.project });
  assert.equal(retained.request.submitted_at, null);
  await cli(preferred, "activate");
  assert.equal((await descriptor()).instance, running.instance);
  await rpc(running, { op: "cancel", id: job.id, project: request.project });
  await rpc(running, { op: "shutdown" });
  await waitForStop(running);
  running = undefined;
  // The fallback launcher must start the preferred executable after shutdown.
  running = JSON.parse((await cli(binary, "connect")).stdout);
  const image = process.platform === "win32"
    ? (await exec("powershell.exe", ["-NoProfile", "-Command", `(Get-Process -Id ${running.pid} -ErrorAction Stop).Path`], { windowsHide: true })).stdout.trim()
    : (await exec("/bin/ps", ["-ww", "-p", String(running.pid), "-o", "comm="])).stdout.trim();
  assert.equal((await realpath(image)).toLowerCase(), (await realpath(preferred)).toLowerCase(), "Fallback launcher started the wrong executable");
  assert.equal((await rpc(running, { op: "health" })).build_id, health.build_id);
  assert.equal((await rpc(running, { op: "get", id: job.id, project: request.project })).request.state, "cancelled");
  await assert.rejects(readFile(join(home, "chrome.json")), { code: "ENOENT" });
  console.log("Runtime lifecycle passed: activation, idempotency, retained unsent request, preferred bootstrap; no Chrome/account submission");
} finally {
  previous?.close();
  if (!running) { try { running = await descriptor(); } catch {} }
  if (running?.pid) {
    await rpc(running, { op: "shutdown" });
    await waitForStop(running);
  }
  await rm(home, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}
