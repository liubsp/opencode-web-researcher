// Keep build-machine paths out of packaged binaries, including dependency panic locations.
import { spawnSync } from "node:child_process";
import { homedir } from "node:os";
import { resolve } from "node:path";
import { createHash } from "node:crypto";
import { readdirSync, readFileSync, statSync } from "node:fs";

const sources = ["Cargo.toml", "Cargo.lock", ...readdirSync("crates", { recursive: true })
  .map(name => `crates/${name.replaceAll("\\", "/")}`)
  .filter(name => /\.(?:rs|js|toml)$/.test(name) && statSync(name).isFile())].sort();
const digest = createHash("sha256");
for (const name of sources) { digest.update(name); digest.update(readFileSync(name)); }
const buildID = digest.digest("hex").slice(0, 16);

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
  env: {...process.env, WEB_RESEARCH_BUILD_ID: buildID, CARGO_ENCODED_RUSTFLAGS: flags.join("\x1f"), CARGO_PROFILE_RELEASE_STRIP: "debuginfo"},
});
if (result.error) throw result.error;
process.exit(result.status ?? 1);
