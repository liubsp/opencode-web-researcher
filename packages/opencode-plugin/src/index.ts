import { Plugin } from "@opencode/plugin";
import { ResearchClient } from "./client.ts";
import { AGENT, PERMISSION, assertResearchAgent, filterTools } from "./isolation.ts";
import { tools } from "./tools.ts";

export default Plugin.define({
  id: "web-research",
  async setup(ctx) {
    const binary = typeof ctx.options.binary === "string" ? ctx.options.binary : process.env.WEB_RESEARCH_BINARY;
    if (!binary) throw new Error("web-research: run the project installer with --binary pointing to web-research");
    const client = new ResearchClient(binary);
    await ctx.agent.transform(editor => {
      for (const agent of editor.list()) {
        const id = String(agent.id);
        editor.update(id, draft => {
          draft.permissions.push({ action: PERMISSION, resource: "*", effect: id === AGENT ? "allow" : "deny" });
        });
      }
    });
    for (const hook of ["context", "generate", "compaction"] as const) {
      await ctx.session.hook(hook, filterTools);
    }
    await ctx.tool.hook("execute.before", event => {
      if (event.tool.startsWith("research_")) assertResearchAgent(event.agent);
    });
    await ctx.tool.transform(editor => {
      for (const tool of tools(ctx.location.project.canonical, input => client.call(input))) editor.add(tool);
    });
  },
});
