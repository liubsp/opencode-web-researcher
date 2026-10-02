import { test } from "node:test";
import assert from "node:assert/strict";
import { Effect, Fiber } from "effect";
import { tools } from "../src/tools.ts";
import { interruptible } from "../src/native-tools.ts";

test("native tool interruption aborts the underlying RPC and stops further wait batches", async () => {
  let started!: () => void;
  const ready = new Promise<void>(resolve => { started = resolve; });
  let calls = 0, aborted = false;
  const definition = tools("synthetic", async (_input, signal) => {
    calls++;
    assert.ok(signal);
    return new Promise((_, reject) => {
      signal.addEventListener("abort", () => { aborted = true; reject(signal.reason); }, { once: true });
      started();
    });
  }).find(tool => tool.name === "research_wait")!;
  const tool = interruptible(definition);
  const context = { agent: "web-researcher", sessionID: "ses_synthetic", id: "call_synthetic", progress: () => Effect.void } as unknown as Parameters<typeof tool.execute>[1];
  const fiber = Effect.runFork(tool.execute({ id: "synthetic", seconds: 300 }, context));
  await ready;
  await Effect.runPromise(Fiber.interrupt(fiber));
  assert.equal(aborted, true);
  assert.equal(calls, 1);
});
