# How it works

The TypeScript plugin is a small bridge between OpenCode and a Rust service. The service owns the
browser and durable job state, so research can outlive an individual tool call.

```text
OpenCode parent agent
  -> web-researcher (inherits the parent session's model by default)
    -> agent-scoped tools
      -> authenticated loopback API
        -> SQLite queue + pacing and budget checks
          -> Chrome DevTools Protocol -> ChatGPT UI
        <- answer, Markdown, citations, request IDs
```

The daemon starts automatically. Opted-in projects share one serialized browser queue, profile,
and config, while ordinary results/listing access is scoped to the calling project. There's no
extension or MCP server. Chrome uses a separate persistent profile and background tabs; renderer
focus emulation keeps response updates working while its window is minimized.

Tool visibility, permissions, and execution-time identity checks restrict research to the agent.
Tools are excluded from Code Mode. This is OpenCode-level isolation, not a sandbox against other
processes running as the same OS user.

## Where to look

| Component | Responsibility |
| --- | --- |
| `packages/opencode-plugin` | Installer, agent instructions, tools, isolation, daemon client |
| `research-core` | Types, config, pacing and budget policy |
| `research-store` | SQLite, reservations, idempotency, saved transcripts |
| `research-browser` | Chrome discovery, ownership checks, background launch, CDP |
| `research-chatgpt` | UI controls, submission, extraction and response matching |
| `research-server` | Local API, scheduler, recovery, cleanup |
| `research-app` | CLI, login, diagnostics and bootstrap |

Submission intent is persisted before clicking Send. Ambiguous submissions aren't automatically
resent. Cleanup verifies saved local transcripts before deleting remote conversations.

## Agent-to-server communication

The model calls ordinary OpenCode tools, not HTTP or shell commands itself. The plugin checks the
agent identity and adds trusted project/session information. On first use it runs the installed
server's internal `connect` command, which discovers or starts the daemon and returns its port,
token, protocol version, and instance ID. These credentials stay in the client, not in model context.

The plugin sends JSON POST requests to `http://127.0.0.1:<port>/v1/rpc` with bearer authentication.
The server checks authorization and Host, rejects browser Origin headers, and persists jobs in
SQLite. It returns stable thread/request IDs immediately; `research_wait` long-polls for up to
60 seconds. Answers, Markdown, citations, and remaining budget come back as JSON tool results.

Follow-ups carry the same thread ID. Submission retries retain the same idempotency key so a
temporary connection failure doesn't create duplicate jobs. The client rediscovers the daemon
after a connection failure or stale authentication. Different opted-in repositories connect to
the same instance while the server scopes ordinary thread access by project.

## Build and check

See [setup](INSTALLATION.md#build-from-a-checkout) for the build steps. Then run:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run check
npm test
npm run build
```

Opt-in Chrome fixtures exercise the real DOM without sending an account message:

```sh
cargo test -p research-chatgpt --test browser_fixture -- --ignored --test-threads=1
```

The live smoke test sends a real prompt with normal pacing. Set `WEB_RESEARCH_LIVE=1`, then run
`node scripts/live-smoke.mjs run`. In PowerShell, set it with `$env:WEB_RESEARCH_LIVE='1'`.
IDs are saved under ignored `target/`. `retire` saves a transcript and deletes only that test's
conversation; `archive` reads its local status. Live account tests aren't part of automatic CI.

The CI workflow targets Windows and macOS and packages the binary and npm tarball. See
[validation status](VALIDATION.md) for actual coverage rather than assuming every target is verified.

Inspired by [ask-bridge](https://github.com/doggy8088/ask-bridge), independently rebuilt in focused
Rust modules. The README and docs directory describe the current behavior and development workflow.
