// Explicit opt-in account integration test. Never part of the unit test suite.
import { execFileSync } from "node:child_process";
import { writeFile, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { ResearchClient } from "../packages/opencode-plugin/dist/client.js";

if (process.env.WEB_RESEARCH_LIVE !== "1") throw new Error("Set WEB_RESEARCH_LIVE=1 to run this account integration test");
const binary = resolve("target/debug", process.platform === "win32" ? "web-research.exe" : "web-research");
const client = new ResearchClient(binary);
const file = resolve("target/live-smoke.json");
const project = "web-research-live-smoke";
const action = process.argv[2] ?? "run";
if (action === "run" || action === "reconcile") {
  const previous = action === "reconcile" ? JSON.parse(await readFile(file, "utf8")) : null;
  const job = action === "reconcile" ? await client.call({op:"reconcile",project,id:previous.id}) : await client.call({ op: "submit", request: { project, session:"live-smoke",
    key:`smoke-${Date.now()}`, prompt:"can you search official chrome docs for remote debugging profile requirements? just short answer and source link pls",
    deep_research:false, thread_id:null } });
  await writeFile(file, JSON.stringify(job, null, 2));
  console.log(`Request ${job.id}, thread ${job.thread_id}; using normal pacing`);
  for (;;) {
    const result = await client.call({op:"wait",project,id:job.id,seconds:60});
    console.log(result.request.state);
    await writeFile(file, JSON.stringify({...result.request,thread_id:job.thread_id}, null, 2));
    if (["completed","failed","cancelled","submission_unknown","needs_attention","timed_out"].includes(result.request.state)) {
      console.log(JSON.stringify(result, null, 2));
      if (result.request.state !== "completed") process.exitCode = 1;
      break;
    }
  }
} else if (action === "retire") {
  const job = JSON.parse(await readFile(file, "utf8"));
  await client.call({op:"retire",project,id:job.thread_id});
  console.log(`Queued archive-before-delete for smoke thread ${job.thread_id}`);
  const deadline = Date.now() + 90_000;
  for (;;) {
    const result = await client.call({op:"archive",project,id:job.thread_id});
    if (result.thread.state === "remote_deleted") {
      const {data_directory} = JSON.parse(execFileSync(binary,["doctor"],{encoding:"utf8"}));
      const markdown = await readFile(resolve(data_directory,"archives",job.thread_id,"thread.md"),"utf8");
      const archive = JSON.parse(await readFile(resolve(data_directory,"archives",job.thread_id,"thread.json"),"utf8"));
      if (!markdown.includes(job.prompt) || !archive.requests.some(r=>r.id===job.id)) throw new Error("Local archive verification failed");
      console.log("Remote conversation deleted; local Markdown and JSON archives verified");
      break;
    }
    if (Date.now() > deadline) throw new Error(result.thread.cleanup_error || "Timed out waiting for cleanup");
    await new Promise(resolve=>setTimeout(resolve,3000));
  }
} else if (action === "archive") {
  const job = JSON.parse(await readFile(file, "utf8"));
  console.log(JSON.stringify(await client.call({op:"archive",project,id:job.thread_id}), null, 2));
} else if (action === "stop") {
  execFileSync(binary, ["shutdown"], {stdio:"inherit"});
} else throw new Error("Expected run, reconcile, retire, archive, or stop");
