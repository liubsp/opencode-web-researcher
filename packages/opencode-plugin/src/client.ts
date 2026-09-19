import { execFile } from "node:child_process";
import { promisify } from "node:util";

const exec = promisify(execFile);
type Descriptor = { port: number; token: string; instance: string; protocol: number };

export class ResearchClient {
  private descriptor?: Promise<Descriptor>;
  constructor(private readonly binary: string) {}

  private connect(): Promise<Descriptor> {
    return this.descriptor ??= exec(this.binary, ["connect"], { timeout: 120_000, windowsHide: true })
      .then(({ stdout }) => {
        const value: Descriptor = JSON.parse(stdout);
        if (value.protocol !== 1 || !Number.isInteger(value.port) || value.port < 1 || value.port > 65535 || !value.token) {
          throw new Error("Invalid research service descriptor");
        }
        return value;
      }).catch(error => { this.descriptor = undefined; throw error; });
  }

  async call(input: Record<string, unknown>): Promise<unknown> {
    for (let attempt = 0; attempt < 2; attempt++) {
      const descriptor = await this.connect();
      let response: Response;
      try {
        response = await fetch(`http://127.0.0.1:${descriptor.port}/v1/rpc`, {
          method: "POST", headers: { authorization: `Bearer ${descriptor.token}`, "content-type": "application/json" },
          body: JSON.stringify(input), signal: AbortSignal.timeout(70_000),
        });
      } catch (error) {
        this.descriptor = undefined;
        if (attempt === 0) continue; // Submit retries preserve the same server idempotency key.
        throw error;
      }
      if (response.status === 401 && attempt === 0) { this.descriptor = undefined; continue; }
      const value = await response.json() as { error?: string };
      if (!response.ok) throw new Error(value.error ?? `Research service returned ${response.status}`);
      return value;
    }
    throw new Error("Research service unavailable");
  }
}
