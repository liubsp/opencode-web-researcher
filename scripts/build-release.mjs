// Keep build-machine paths out of packaged binaries, including dependency panic locations.
import { spawnSync } from "node:child_process";
import { homedir } from "node:os";
import { resolve } from "node:path";

const mappings = [
  [homedir(), "/user"],
  [process.env.CARGO_HOME || resolve(homedir(), ".cargo"), "/cargo"],
  [process.env.RUSTUP_HOME || resolve(homedir(), ".rustup"), "/rustup"],
  [process.cwd(), "/src/opencode-web-researcher"],
];
const flags = process.env.CARGO_ENCODED_RUSTFLAGS?.split("\x1f")
  ?? (process.env.RUSTFLAGS || "").split(/\s+/).filter(Boolean);
for (const [from, to] of mappings) {
  for (const prefix of new Set([from, from.replaceAll("\\", "/")])) {
    flags.push(`--remap-path-prefix=${prefix}=${to}`);
  }
}
const result = spawnSync("cargo", ["build", "--release", "--locked", "-p", "research-app"], {
  stdio: "inherit",
  env: {...process.env, CARGO_ENCODED_RUSTFLAGS: flags.join("\x1f"), CARGO_PROFILE_RELEASE_STRIP: "debuginfo"},
});
if (result.error) throw result.error;
process.exit(result.status ?? 1);
