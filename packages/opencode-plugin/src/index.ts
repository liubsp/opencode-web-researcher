import { Plugin } from "@opencode/plugin";
import { ResearchClient } from "./client.ts";
import { AGENT, PERMISSION, assertResearchAgent, filterTools } from "./isolation.ts";
import { tools } from "./tools.ts";
import { discoverTempDirectory, publishTranscripts, cleanTempTranscripts } from "./transcripts.ts";

export default Plugin.define({
  id: "web-research",
  async setup(ctx) {
    const binary = typeof ctx.options.binary === "string" ? ctx.options.binary : process.env.WEB_RESEARCH_BINARY;
    if (!binary) throw new Error("web-research: run the project installer with --binary pointing to web-research");
    const client = new ResearchClient(binary);
    // Resolve lazily: a failed temp export must not prevent registration or research work.
    let temporary: Promise<string> | undefined;
    const call = async (input: Record<string, unknown>) => {
      const result = await client.call(input);
      const hasTranscript = result && typeof result === "object" && ("local_transcript" in result || "results" in result);
      if (!hasTranscript) return result;
      temporary ??= discoverTempDirectory().catch(error => { temporary = undefined; throw error; });
      try {
        const directory = await temporary;
        await cleanTempTranscripts(directory);
        return await publishTranscripts(result, directory);
      } catch (error) {
        // Keep results usable, but don't advertise inaccessible source paths as shared exports.
        const data = result as Record<string, unknown>;
        for (const entry of [data, ...(Array.isArray(data.results) ? data.results : [])]) {
          if (entry && typeof entry === "object" && "local_transcript" in entry) entry.local_transcript = {markdown: null, error: String(error)};
        }
        return data;
      }
    };
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
      for (const tool of tools(ctx.location.project.canonical, call)) editor.add(tool);
    });
  },
});
