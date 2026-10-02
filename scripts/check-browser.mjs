// One isolated background Chrome per suite, never the installed research profile.
import { spawnSync } from "node:child_process";
import { mkdtemp, writeFile, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const home = await mkdtemp(join(tmpdir(), "research-fixture-"));
await writeFile(join(home, "fixture-owned"), "Synthetic browser fixtures only\n");
let result;
try {
  result = spawnSync("cargo", ["test", "-p", "research-chatgpt", "--test", "browser_fixture", "--locked", "--", "--ignored", "--test-threads=1", "--nocapture"], {
    stdio: "inherit", env: { ...process.env, WEB_RESEARCH_FIXTURE_HOME: home },
    timeout: 180_000, killSignal: "SIGKILL",
  });
  if (result.error) throw result.error;
} finally {
  let record;
  try { record = JSON.parse(await readFile(join(home, "chrome.json"), "utf8")); }
  catch (error) { if (error.code !== "ENOENT") throw error; }
  if (record) {
    const endpoint = new URL(record.websocket);
    if (endpoint.hostname !== "127.0.0.1" || Number(endpoint.port) !== record.port) throw new Error("Unexpected fixture browser identity");
    await new Promise((resolve, reject) => {
      const socket = new WebSocket(record.websocket);
      const timeout = setTimeout(() => { socket.close(); reject(new Error("Fixture browser close timed out")); }, 5000);
      const disconnected = () => { clearTimeout(timeout); resolve(); };
      socket.onopen = () => socket.send(JSON.stringify({ id: 1, method: "Browser.close", params: {} }));
      socket.onclose = disconnected;
      // Chromium may drop CDP abruptly while closing; endpoint shutdown below is the evidence.
      socket.onerror = disconnected;
    });
    // Browser.close can acknowledge before file handles have drained.
    for (let attempt = 0; attempt < 40; attempt++) {
      try { await fetch(`http://127.0.0.1:${record.port}/json/version`, { signal: AbortSignal.timeout(1000) }); }
      catch { break; }
      if (attempt === 39) throw new Error("Fixture browser did not close");
      await new Promise(resolve => setTimeout(resolve, 250));
    }
  }
  await rm(home, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}
process.exit(result.status ?? 1);
