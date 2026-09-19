export const AGENT = "web-researcher";
export const PERMISSION = "web_research";
export const PREFIX = "research_";

export function assertResearchAgent(agent: string): void {
  if (agent !== AGENT) throw new Error("Research tools are restricted to the web-researcher agent");
}

export function filterTools(event: { agent: string; tools: Record<string, unknown> }): void {
  if (event.agent === AGENT) return;
  for (const name of Object.keys(event.tools)) {
    if (name.startsWith(PREFIX)) delete event.tools[name];
  }
}
