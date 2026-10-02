import { Effect } from "effect";
import type { Info } from "@opencode/plugin/promise/tool";
import { Error as ToolError } from "@opencode/plugin/promise/tool";
import type { ToolEditor } from "@opencode/plugin/effect/tool";

// The pinned Promise adapter does not forward its Fiber's cancellation signal.
// Use the native Effect boundary while retaining the existing Promise executors.
export function interruptible(tool: Info): Parameters<ToolEditor["add"]>[0] {
  return {
    ...tool,
    execute: (input, context) => Effect.tryPromise({
      try: signal => {
        const execution = { ...context, signal, progress: (update: Parameters<typeof context.progress>[0]) => Effect.runPromise(context.progress(update)) };
        return tool.execute(input, execution);
      },
      catch: error => error instanceof ToolError ? error : new ToolError({ message: error instanceof Error ? error.message : String(error), error }),
    }),
  };
}
