// One isolated background Chrome per suite, never the installed research profile.
import { spawn, spawnSync } from "node:child_process";
import { mkdtemp, mkdir, writeFile, readFile, rm, realpath } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createServer } from "node:net";
import { pathToFileURL } from "node:url";

const home = await mkdtemp(join(tmpdir(), "research-fixture-"));
await writeFile(join(home, "fixture-owned"), "Synthetic browser fixtures only\n");
let result;
let fixtureProcess;
try {
  if (process.platform === "darwin") {
    // Hosted runners cannot reliably launch apps through LaunchServices. Exercise the same normal
    // Chrome/CDP/DOM path, but do not claim these fixtures verify nonactivating desktop app launch.
    await mkdir(join(home, "chrome-profile"));
    const profile = await realpath(join(home, "chrome-profile"));
    const marker = join(home, "fixture.html");
    await writeFile(marker, "<!doctype html><title>Synthetic fixture</title>");
    const markerURL = pathToFileURL(await realpath(marker)).href;
    const reservation = createServer();
    await new Promise(resolve => reservation.listen(0, "127.0.0.1", resolve));
    const port = reservation.address().port;
    await new Promise(resolve => reservation.close(resolve));
    fixtureProcess = spawn("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", [
      `--user-data-dir=${profile}`, `--remote-debugging-port=${port}`,
      "--remote-debugging-address=127.0.0.1", "--no-first-run", "--no-default-browser-check",
      "--start-minimized", "--window-position=-2000,-2000", markerURL,
    ], { stdio: "ignore" });
    let launchError;
    fixtureProcess.on("error", error => { launchError = error; });
    let ready = false;
    for (let attempt = 0; attempt < 60; attempt++) {
      if (launchError) throw launchError;
      if (fixtureProcess.exitCode !== null) throw new Error(`Fixture Chrome exited (${fixtureProcess.exitCode})`);
      try {
        const endpoint = `http://127.0.0.1:${port}`;
        const targets = await (await fetch(`${endpoint}/json/list`, { signal: AbortSignal.timeout(1000) })).json();
        if (targets.some(target => target.url === markerURL)) {
          const version = await (await fetch(`${endpoint}/json/version`, { signal: AbortSignal.timeout(1000) })).json();
          const websocket = version.webSocketDebuggerUrl;
          const identity = new URL(websocket);
          if (identity.protocol !== "ws:" || identity.hostname !== "127.0.0.1" || Number(identity.port) !== port || !identity.pathname.startsWith("/devtools/browser/")) throw new Error("Unexpected fixture endpoint");
          await writeFile(join(home, "chrome.json"), JSON.stringify({ port, websocket, profile: await realpath(profile) }));
          ready = true;
          break;
        }
      } catch (error) { if (error.message === "Unexpected fixture endpoint") throw error; }
      await new Promise(resolve => setTimeout(resolve, 250));
    }
    if (!ready) throw new Error("Direct fixture Chrome did not expose its owned endpoint");
    console.log("macOS fixtures: isolated normal Chrome; LaunchServices desktop startup is not covered");
  }
  result = spawnSync("cargo", ["test", "-p", "research-chatgpt", "--test", "browser_fixture", "--locked", "--", "--ignored", "--test-threads=1", "--nocapture"], {
    stdio: "inherit", env: { ...process.env, WEB_RESEARCH_FIXTURE_HOME: home },
    timeout: 180_000, killSignal: "SIGKILL",
  });
  if (result.error) throw result.error;
} finally {
  if (!result && fixtureProcess?.exitCode === null) fixtureProcess.kill();
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
