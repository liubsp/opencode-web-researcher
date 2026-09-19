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
