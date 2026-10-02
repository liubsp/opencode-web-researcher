import { test } from "node:test";
import assert from "node:assert/strict";
import { ResearchClient } from "../src/client.ts";

const descriptor = { protocol: 1, port: 12345, token: "synthetic", instance: "synthetic" };

test("cancellation during shared startup never proceeds to submission", async t => {
  const controller = new AbortController();
  const client = new ResearchClient("unused-test-binary");
  let finish!: (value: typeof descriptor) => void;
  Object.assign(client, { descriptor: new Promise(resolve => { finish = resolve; }) });
  let calls = 0;
  t.mock.method(globalThis, "fetch", async () => { calls++; return new Response("{}"); });
  const pending = client.call({ op: "submit" }, controller.signal);
  controller.abort();
  finish(descriptor);
  await assert.rejects(pending, { name: "AbortError" });
  assert.equal(calls, 0);
});

test("transport cancellation is never retried", async t => {
  const controller = new AbortController();
  const client = new ResearchClient("unused-test-binary");
  Object.assign(client, { descriptor: Promise.resolve(descriptor) });
  let calls = 0;
  t.mock.method(globalThis, "fetch", async (_url: unknown, options: RequestInit) => {
    calls++;
    controller.abort();
    assert.ok(options.signal?.aborted);
    throw controller.signal.reason;
  });
  await assert.rejects(client.call({ op: "submit" }, controller.signal), { name: "AbortError" });
  assert.equal(calls, 1);
});
