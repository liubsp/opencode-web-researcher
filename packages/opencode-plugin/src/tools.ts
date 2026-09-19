import type { Info } from "@opencode/plugin/promise/tool";
import { assertResearchAgent, PERMISSION } from "./isolation.ts";

type Call = (input: Record<string, unknown>) => Promise<unknown>;
const text = { type: "string", minLength: 1, maxLength: 32000 };
const id = { type: "string", minLength: 1, maxLength: 256 };
const object = (properties: Record<string, unknown>, required: string[] = []) => ({ type: "object", properties, required, additionalProperties: false });

export function tools(project: string, call: Call): Info[] {
  const definitions = [
    { name: "start", description: "Start one ChatGPT research thread in normal chat mode; ChatGPT decides when to search. Only an explicit user request for Deep Research may enable deep_research. Save the returned IDs. Reuse request_key for retries.",
      input: object({ prompt: text, request_key: id, deep_research: { type: "boolean", default: false } }, ["prompt", "request_key"]) },
    { name: "send", description: "Submit a useful follow-up in the same thread. Hard limit: ten prompts total. Reuse request_key for retries. Never send while a request is pending.",
      input: object({ thread_id: id, prompt: text, request_key: id }, ["thread_id", "prompt", "request_key"]) },
    { name: "wait", description: "Wait patiently for an existing request. Repeat while pending, without resubmitting. Queue/composition time is additional to 1–10 minute Search; Deep Research can take longer.",
      input: object({ id, seconds: { type: "integer", minimum: 1, maximum: 60, default: 60 } }, ["id"]) },
    { name: "get", description: "Read request state, partial/final response and remaining prompt budget.", input: object({ id }, ["id"]) },
    { name: "list", description: "List this project's active research chats, or locally saved transcripts when requested. This does not list ChatGPT's archived chats.",
      input: object({ archived: { type: "boolean" }, all_projects: { type: "boolean" } }) },
    { name: "resume", description: "Resume an unexpired thread by ID without sending a prompt. Never use a replacement thread to evade its prompt limit.", input: object({ id }, ["id"]) },
    { name: "archive", description: "Read a saved local transcript and its results, including after ChatGPT deletion. This does not use ChatGPT's Archive chat feature.", input: object({ id }, ["id"]) },
    { name: "cancel", description: "Cancel queued work or request ChatGPT to stop an active response. A submitted prompt still counts.", input: object({ id }, ["id"]) },
    { name: "reconcile", description: "After the user fixes login/browser issues, observe an ambiguous submission again without resending. Only use when explicitly told the issue is fixed.", input: object({ id }, ["id"]) },
  ];
  return definitions.map(definition => ({
    ...definition,
    name: `research_${definition.name}`,
    // Intentionally exclude these tools from Code Mode's global catalog. Only direct agent-scoped tools are used.
    options: { codemode: false, permission: PERMISSION },
    execute: async (raw, context) => {
      assertResearchAgent(context.agent);
      const input = raw as Record<string, unknown>;
      await context.progress({ status: definition.name === "wait" ? "Waiting for research" : "Contacting research service" });
      const body = ["start", "send"].includes(definition.name)
        ? { op: "submit", request: { project, session: context.sessionID,
            key: `${context.sessionID}:${input.request_key}`, thread_id: input.thread_id ?? null,
            prompt: input.prompt, deep_research: input.deep_research ?? false } }
        : { ...input, op: definition.name, project };
      const value = await call(body);
      return { content: JSON.stringify(value), metadata: { research: true } };
    },
  }));
}
