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
    { name: "get", description: "Read request state, partial/final response and remaining prompt budget; for read-only imports, returns per-chat result metadata for research_read_content.", input: object({ id }, ["id"]) },
    { name: "list", description: "List this project's active research chats, or locally saved transcripts when requested. This does not list ChatGPT's archived chats.",
      input: object({ archived: { type: "boolean" }, all_projects: { type: "boolean" } }) },
    { name: "resume", description: "Resume an unexpired thread by ID without sending a prompt. Never use a replacement thread to evade its prompt limit.", input: object({ id }, ["id"]) },
    { name: "archive", description: "Read a saved local transcript and its results, including after ChatGPT deletion. This does not use ChatGPT's Archive chat feature.", input: object({ id }, ["id"]) },
    { name: "cancel", description: "Cancel queued work or request ChatGPT to stop an active response. A submitted prompt still counts.", input: object({ id }, ["id"]) },
    { name: "reconcile", description: "After the user fixes login/browser issues, observe an ambiguous submission again without resending. Only use when explicitly told the issue is fixed.", input: object({ id }, ["id"]) },
    { name: "read_chats", description: "Read 1–10 user-specified ChatGPT conversation IDs or URLs, without sending prompts or deleting/modifying those chats. Returns a read request ID; use research_wait/get, then research_read_content for each result. Only accessible chats in the signed-in account can be read; capture covers the rendered current branch, not a guaranteed full export. Reuse request_key for retries.",
      input: object({ chats: { type: "array", items: { type: "string", minLength: 1, maxLength: 2048 }, minItems: 1, maxItems: 10 }, request_key: id }, ["chats", "request_key"]) },
    { name: "read_content", description: "Read a saved imported chat's Markdown in character-offset pages. Use the read request ID and zero-based chat_index from research_wait/get. Follow next_offset until null. Does not reopen Chrome or send prompts.",
      input: object({ id, chat_index: { type: "integer", minimum: 0, maximum: 9 }, offset: { type: "integer", minimum: 0, default: 0 }, limit: { type: "integer", minimum: 1, maximum: 64000, default: 20000 } }, ["id", "chat_index"]) },
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
        : definition.name === "read_chats"
          ? { ...input, op: "read_chats", project, session: context.sessionID, request_key: `${context.sessionID}:${input.request_key}` }
          : { ...input, op: definition.name, project };
      const value = await call(body);
      return { content: JSON.stringify(value), metadata: { research: true } };
    },
  }));
}
