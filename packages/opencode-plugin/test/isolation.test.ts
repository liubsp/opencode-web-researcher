import { test } from "node:test";
import assert from "node:assert/strict";
import { filterTools, assertResearchAgent } from "../src/isolation.ts";
import { tools } from "../src/tools.ts";
import type { ToolContext } from "@opencode/plugin/promise/tool";

test("other agents cannot see research tools and research requests do not mutate another snapshot", () => {
  const original = { research_start: {}, research_wait: {}, read: {}, execute: {} };
  const parent = { agent: "build", tools: { ...original } };
  const research = { agent: "web-researcher", tools: { ...original } };
  filterTools(parent);
  filterTools(research);
  assert.deepEqual(Object.keys(parent.tools), ["read", "execute"]);
  assert.deepEqual(research.tools, original);
  for (const agent of ["general", "explore", "plan", "", "Web-Research"]) {
    assert.throws(() => assertResearchAgent(agent), /restricted/);
  }
});

test("direct invocation is rejected before any service call; tools are excluded from Code Mode", async () => {
  let calls = 0;
  const definitions = tools("project-a", async () => { calls++; return {}; });
  for (const tool of definitions) {
    assert.equal(tool.options?.codemode, false);
    await assert.rejects(tool.execute({}, { agent: "build" } as ToolContext), /restricted/);
  }
  assert.equal(calls, 0);
});

test("trusted execution context supplies project/session; repeated submissions keep their key", async () => {
  const calls: Record<string, unknown>[] = [];
  const tool = tools("project-a", async input => { calls.push(input); return { id: "job" }; })[0];
  const ctx = { agent: "web-researcher", sessionID: "session-a", progress: async () => {} } as unknown as ToolContext;
  const input = { prompt: "check docs pls", request_key: "message-1", project: "forged", session: "forged" };
  await tool.execute(input, ctx);
  await tool.execute(input, ctx);
  assert.deepEqual(calls[0], calls[1]);
  assert.deepEqual(calls[0].request, { project: "project-a", session: "session-a", key: "session-a:message-1",
    thread_id: null, prompt: "check docs pls", deep_research: false });
});

test("read-only imports retain trusted scope and stable retry keys", async () => {
  const calls: Record<string, unknown>[] = [];
  const tool = tools("project-a", async input => { calls.push(input); return {}; }).find(t => t.name === "research_read_chats")!;
  const ctx = { agent: "web-researcher", sessionID: "session-a", progress: async () => {} } as unknown as ToolContext;
  const input = { chats: ["00000000-0000-4000-8000-000000000001"], request_key: "read-1", project: "forged", session: "forged" };
  await tool.execute(input, ctx);
  await tool.execute(input, ctx);
  assert.deepEqual(calls[0], calls[1]);
  assert.equal(calls[0].project, "project-a");
  assert.equal(calls[0].session, "session-a");
  assert.equal(calls[0].request_key, "session-a:read-1");
  assert.equal(calls[0].op, "read_chats");
});

test("saved managed responses can be paged without submitting a new prompt", async () => {
  const calls: Record<string, unknown>[] = [];
  const tool = tools("project-a", async input => { calls.push(input); return { markdown: "part", next_offset: 4 }; })
    .find(t => t.name === "research_response_content")!;
  const ctx = { agent: "web-researcher", sessionID: "session-a", progress: async () => {} } as unknown as ToolContext;
  const result = await tool.execute({ id: "managed-request", offset: 0, limit: 4, project: "forged" }, ctx);
  assert.deepEqual(calls, [{ op: "response_content", id: "managed-request", offset: 0, limit: 4, project: "project-a" }]);
  const content = result.content;
  assert.ok(typeof content === "string");
  assert.match(content, /"next_offset":4/);
});

test("health exposes blocked cleanup without changing trusted scope or submitting work", async () => {
  const calls: Record<string, unknown>[] = [];
  const status = { service_status: { state: "attention_required", cleanup: { blocked: [{ error: "deletion_access_unavailable" }] } } };
  const tool = tools("project-a", async input => { calls.push(input); return status; }).find(t => t.name === "research_health")!;
  const context = { agent: "web-researcher", sessionID: "session-a", progress: async () => {} } as unknown as ToolContext;
  const result = await tool.execute({ project: "forged" }, context);
  assert.deepEqual(calls, [{ op: "health", project: "project-a" }]);
  assert.deepEqual(JSON.parse(result.content as string), status);
});

test("one default wait batches short RPCs without returning to the agent every minute", async t => {
  let clock = 0;
  t.mock.method(Date, "now", () => clock);
  const calls: Record<string, unknown>[] = [];
  const tool = tools("project-a", async input => {
    calls.push(input);
    clock += Number(input.seconds) * 1000;
    return { request: { state: "waiting" } };
  }).find(t => t.name === "research_wait")!;
  const ctx = { agent: "web-researcher", sessionID: "session-a", progress: async () => {} } as unknown as ToolContext;
  await tool.execute({ id: "request" }, ctx);
  assert.equal(clock, 300_000);
  assert.equal(calls.length, 5);
  assert.ok(calls.every(c => c.op === "wait" && c.seconds === 60 && c.project === "project-a"));
  calls.length = 0;
  await tool.execute({ id: "request", seconds: 45 }, ctx);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].seconds, 45);
  calls.length = 0;
  await tool.execute({ id: "request", seconds: 600 }, ctx);
  assert.equal(calls.length, 10);
});

test("wait returns immediately on completion, failure, timeout, or required attention", async () => {
  for (const state of ["completed", "failed", "cancelled", "timed_out", "submission_unknown", "needs_attention"]) {
    let calls = 0;
    const tool = tools("project-a", async () => {
      calls++;
      return { request: { state } };
    }).find(t => t.name === "research_wait")!;
    const ctx = { agent: "web-researcher", sessionID: "session-a", progress: async () => {} } as unknown as ToolContext;
    await tool.execute({ id: "request" }, ctx);
    assert.equal(calls, 1);
  }
});

test("import waits use the same default interval and stop on a completed capture", async t => {
  let clock = 0;
  t.mock.method(Date, "now", () => clock);
  let calls = 0;
  const tool = tools("project-a", async input => {
    clock += Number(input.seconds) * 1000;
    return { state: ++calls === 2 ? "completed" : "reading" };
  }).find(t => t.name === "research_wait")!;
  const ctx = { agent: "web-researcher", sessionID: "session-a", progress: async () => {} } as unknown as ToolContext;
  await tool.execute({ id: "read-request" }, ctx);
  assert.equal(calls, 2);
  assert.equal(clock, 120_000);
});

test("aborted tool contexts cannot submit and interrupted waits stop batching", async () => {
  const controller = new AbortController();
  controller.abort();
  let calls = 0;
  const context = { agent: "web-researcher", sessionID: "session-a", progress: async () => {}, signal: controller.signal } as unknown as ToolContext;
  const start = tools("project-a", async () => { calls++; return {}; })[0];
  await assert.rejects(start.execute({ prompt: "question", request_key: "key" }, context), { name: "AbortError" });
  assert.equal(calls, 0);
  const waiting = new AbortController();
  const wait = tools("project-a", async (_input, signal) => {
    assert.equal(signal, waiting.signal);
    calls++;
    waiting.abort();
    return { request: { state: "waiting" } };
  }).find(t => t.name === "research_wait")!;
  await assert.rejects(wait.execute({ id: "request" }, { ...context, signal: waiting.signal } as unknown as ToolContext), { name: "AbortError" });
  assert.equal(calls, 1);
});
