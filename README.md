# web-research-opencode

Project-local OpenCode V2 research agent backed by ChatGPT in installed Google Chrome.
Rust owns the background service, browser connection, durable queue, timing, chat limits,
and saved local transcripts. A small TypeScript plugin exposes tools **only to the research agent**.

[Overview](#purpose-and-overview) · [Architecture](#architecture) · [Build](#build) ·
[Install](#install-in-a-chosen-project) · [Usage](#use) · [Configuration](#shared-configuration-and-storage) ·
[Troubleshooting](#diagnostics-and-recovery) · [Development](#development-checks)

## Purpose and overview

Use this when an OpenCode coding agent needs patient, sourced web research through your signed-in
ChatGPT account. Delegate the question to **`web-researcher`**; it composes the questions, waits for
ChatGPT, asks useful follow-ups in the same conversation, and returns findings with source links.
It uses ChatGPT's website, not the OpenAI API, and does not require an OpenAI API key. You still
need a ChatGPT account with access to the selected model, reasoning level, and research mode.

Installation is **per project**. The daemon, Chrome profile/login, configuration, queue, and saved
transcripts are **shared per OS user** across projects that opt in. The agent's normal list/results
access is scoped to its project. Research messages are sent to ChatGPT and local transcripts are
stored on disk; consider that when choosing what project context to include in a question.

### Architecture

```text
OpenCode parent agent
  -> project-local web-researcher subagent
    -> agent-scoped TypeScript research tools
      -> authenticated loopback HTTP API (auto-started Rust daemon)
        -> SQLite jobs + shared serial worker + pacing/budget policies
          -> Chrome DevTools Protocol (CDP)
            -> separate persistent Chrome profile -> ChatGPT UI
        <- response text, Markdown, citations, and durable request IDs
        -> local transcript files before expired/capped conversation deletion
```

The TypeScript layer enforces agent identity and carries trusted project/session information.
Rust owns durable state and the global work queue. The browser adapter launches/reconnects to the
managed Chrome instance; the ChatGPT adapter handles composer controls, submission, response
matching, and deletion. No browser extension or MCP server is required.

| Component | Responsibility |
| --- | --- |
| `packages/opencode-plugin` | Project installer, agent instructions, tool registration, isolation, daemon client |
| `research-core` | Shared types, configuration, timing and budget policy |
| `research-store` | SQLite persistence, transactional reservations, idempotency, saved transcripts |
| `research-browser` | Chrome discovery, ownership verification, minimized launch, CDP transport |
| `research-chatgpt` | ChatGPT UI actions, mode/reasoning selection, response extraction and attribution |
| `research-server` | Authenticated local API, scheduling, recovery, lifecycle and cleanup |
| `research-app` | Executable, configuration, login, diagnostics and daemon bootstrap |

See [PLAN.md](PLAN.md) for design details and [docs/VALIDATION.md](docs/VALIDATION.md) for verified coverage.

## Behavior

- Normal ChatGPT chat by default; ChatGPT decides when to search. Deep Research is enabled only
  on an explicit user request, directly or relayed by the parent agent, never from complexity alone.
- Persistent research Chrome profile: sign in once, reuse across projects. No extension,
  headless browser, `--enable-automation`, or port-0 debugging launch.
- Research stays minimized and opens background tabs without taking focus. Only the explicit
  `login` command restores the window. Renderer-only focus emulation keeps answer rendering alive.
- Automatic daemon startup on the first research tool call.
- Shared model settings: account default model, Extra High when available, otherwise High.
- Message composition delay: **40 words/minute + 15 seconds**, applied by the server.
- At most **10 prompts per thread**, including the first; one active browser request globally.
- Save a local transcript before deleting the ChatGPT conversation after **24 hours idle** or the
  tenth response. Local Markdown/JSON copies remain available indefinitely.
- Stable IDs, idempotent retries, resumable waiting, and explicit ambiguous-submission states.
- A dedicated subagent writes concise, casual broken-English research messages. Parent agents
  receive its sourced summary, not access to the underlying research tools.

## Build

Requirements: installed Google Chrome, Rust stable (edition 2024), Node.js 24+, and OpenCode **2.0.8**.
The intended release targets are Windows and macOS. Development/live checks currently run on Windows;
the supplied CI matrix targets both platforms, but macOS live behavior has not been verified.

```sh
cargo build --release -p research-app
npm ci
npm run build
```

The executable is `target/release/web-research.exe` on Windows and `target/release/web-research`
on macOS. CI packages the executable and an npm tarball; prebuilt users do not need Rust.
This repository has not yet been published to npm.

## Install in a chosen project

From this checkout, after building:

```powershell
# Windows — use the absolute executable path when installing into another project.
node packages/opencode-plugin/dist/install.js --project "C:\projects\my-project" --binary "C:\tools\web-research-opencode\target\release\web-research.exe"
.\target\release\web-research.exe configure
.\target\release\web-research.exe login
```

```sh
# macOS
node packages/opencode-plugin/dist/install.js --project /path/to/project --binary /path/to/web-research/target/release/web-research
./target/release/web-research configure
./target/release/web-research login
```

Sign in in the research Chrome window. This is a separate persistent Chrome profile, so your
everyday Chrome login is not automatically inherited. Reload the project's OpenCode location
after installation. Repeat the installer for each project you want to opt in.

To install into this checkout, use `--project .`. For development, `cargo build -p research-app`
and `target/debug/web-research[.exe]` can replace the release build and executable paths above.
Use `web-research doctor` and `web-research browser-check` to verify the configuration and browser.
In OpenCode, confirm that `web-researcher` is available after reloading the project.

The installer preserves existing JSONC settings/comments, adds a project plugin entry, and
installs `.opencode/agents/web-researcher.md`. It refuses to overwrite a customized agent.
It does **not** register a global OpenCode plugin or agent. Keep the plugin checkout/package and
configured executable at their installed paths.

For CI/prebuilt artifacts, install the provided npm tarball in a stable tools directory, then use
its `web-research-setup` command with `--project` and `--binary` pointing to the downloaded executable.

```sh
# Run in your chosen permanent tools directory; substitute the downloaded filenames/paths.
npm install /path/to/web-research-opencode-0.1.0.tgz
npx --no-install web-research-setup --project /path/to/project --binary /path/to/web-research
```

On Windows, use the `.exe` binary and quote paths containing spaces. Run that binary's `configure`
and `login` commands as in the source-install instructions. Prebuilt installation still needs
Node.js, OpenCode, and installed Google Chrome. Download a binary matching your OS and architecture.

### Example: install into another Git repository

Run these commands **from the plugin checkout**, with the target repository already present:

```powershell
# Windows example: plugin checkout C:\tools\web-research-opencode
# Target Git repository C:\projects\my-app
cargo build --release -p research-app
npm ci
npm run build
node packages/opencode-plugin/dist/install.js --project "C:\projects\my-app" --binary "C:\tools\web-research-opencode\target\release\web-research.exe"
```

On macOS, use the same installer with absolute POSIX paths and the executable without `.exe`.
Sign in once using the executable's `login` command if needed. Open the target repository in
OpenCode and reload its location (or reopen it), then ask:

> use web-researcher to compare the current supported releases of this dependency using official sources

The researcher uses Astra Low inside OpenCode and delegates the research to your shared ChatGPT
profile. To use Deep Research, explicitly say “use web-researcher with Deep Research”. A ChatGPT
project configured in the machine-wide config applies here too; it is independent of the Git repo.

#### Effects on the target repository

- Adds or updates `opencode.jsonc` (or existing `opencode.json`) with a plugin entry containing
  absolute paths to this plugin package and executable. Existing settings/comments are preserved.
- Creates `.opencode/agents/web-researcher.md`. A differing existing file causes an installation
  conflict rather than being overwritten. The installer refuses ambiguous dual JSON/JSONC configs.
- These files appear in `git status`; the installer does not commit or push. Absolute local paths
  are machine-specific. For a local-only setup, add those paths to `.git/info/exclude` when they are
  untracked. Excluding a file does not hide changes to an already tracked configuration file; review
  such changes before committing. Teammates should run the installer with their own paths.
- Does not install npm dependencies into the target repo or modify its application source,
  dependency manifest, lockfile, remotes, or Git hooks. Build/install dependencies stay in the
  plugin checkout (or permanent tools directory when using the packaged artifact).
- Loads a plugin when OpenCode opens this opted-in location. Research tools are restricted to
  `web-researcher`; the plugin filters those tools from other agents and denies their research
  permission. The research agent's tool permissions deny file edits and shell commands.
- All opted-in repos share one daemon, Chrome login, ChatGPT model/project settings, and serialized
  queue. Work in one repo may delay another. Machine-wide config changes affect future requests
  across repos. Stopping the daemon interrupts shared service availability until it starts again.
- Research prompts go to ChatGPT, use that account's limits, and produce stored local transcripts.
  Automatic cleanup deletes only managed research conversations after saving their transcripts;
  it does not delete repository files. The parent coding agent still has its usual permissions.

To remove it from this repo, remove its plugin entry and agent file, then reload the OpenCode
location. Shared machine data remains available to other opted-in repos.

### Updating or removing an installation

Before updating the executable on Windows, run `web-research shutdown`. Rebuild the Rust executable
and plugin, rerun the installer if its path or registration changed, and reload OpenCode's project.
If the packaged agent instructions have changed, compare them with the installed file and update
it deliberately: the installer treats differing content as a conflict, even for an older version.
For installations using the old `web-research` agent name, replace that installed definition with
`web-researcher.md` after reviewing any customizations.

To uninstall from one project, remove its plugin entry from `opencode.json(c)` and its installed
`.opencode/agents/web-researcher.md`, then reload that project. This does not erase the shared login,
configuration, or transcripts used by other opted-in projects. `web-research shutdown` stops the
shared daemon; another opted-in project can start it again on its next research call.

## Use

Ask your primary OpenCode agent:

The OpenCode `web-researcher` subagent uses **GPT-6 Astra, low reasoning**
(`model: openai/gpt-6-astra#low` in its Markdown frontmatter). This controls the agent orchestrating
research; the ChatGPT website's model and reasoning level use the separate shared server settings.

> use the web-researcher agent to compare current Rust CDP libraries, official sources pls

For the optional longer workflow:

> use the web-researcher agent with Deep Research to investigate this question …

For multi-step research, make the verification goal explicit:

> use web-researcher to compare these options using official sources, then verify the weakest
> claim with one follow-up in the same chat. include source URLs and unresolved uncertainties.

To continue earlier work, give the agent the thread ID from its report and ask it to resume that
thread. A useful follow-up consumes another prompt; reading results or waiting does not.
The agent normally aims for one initial question and zero to two follow-ups. The ten-prompt limit
is a ceiling, not a target. Completed threads remain resumable until expiry or cleanup.

Expect the first request to include queue time, the enforced composition delay, and ChatGPT's own
research time. The agent should wait on the existing request rather than send reminders or restart
the conversation. Its messages to ChatGPT use understandable casual broken English; its report to
you uses clear prose with source links, caveats, and thread/request IDs.

Links supplied by ChatGPT are attributed as such. They are not automatically equivalent to sources
independently opened and verified by the research agent.

The research agent has `research_start`, `research_send`, `research_wait`, `research_get`,
`research_list`, `research_resume`, `research_archive`, `research_cancel`, and `research_reconcile`.
Other agents are denied the custom permission, their model contexts are filtered, and execution
checks the trusted agent identity. Research tools are excluded from Code Mode's catalog.

| Tool | Use |
| --- | --- |
| `research_start` | Start a thread; use a stable request key for retries |
| `research_send` | Send a new follow-up in an existing thread |
| `research_wait` / `research_get` | Wait up to 60 seconds per call / inspect current or completed results |
| `research_list` / `research_resume` | Find threads / access an unexpired thread without sending |
| `research_archive` | Read saved local transcripts; unrelated to ChatGPT's Archive chat feature |
| `research_cancel` | Cancel queued work or request a stop for an active response |
| `research_reconcile` | Observe an ambiguous submission after its underlying issue is fixed; never resend |

This is OpenCode-level agent isolation, not a sandbox against arbitrary processes running as the
same OS user. Research is intentionally not exposed as a shell CLI workflow.

## Shared configuration and storage

`web-research configure` creates the shared `config.json` and prints its path.
`web-research doctor` reports paths, Chrome discovery, and service status without starting research.
Default user-data directories:

- Windows: `%LOCALAPPDATA%\web-research-opencode`
- macOS: `~/Library/Application Support/web-research-opencode`
- Override for testing with `WEB_RESEARCH_HOME` (the plugin and executable must inherit the same value).

```json
{
  "model": "default",
  "chatgpt_project_url": null,
  "reasoning_preferences": ["Extra High", "High"],
  "words_per_minute": 40,
  "fixed_pause_seconds": 15,
  "inactivity_hours": 24,
  "search_timeout_seconds": 900,
  "deep_research_timeout_seconds": 3600,
  "chrome_path": null
}
```

Model names are ChatGPT UI labels, not API model IDs. Settings are reloaded for newly admitted
requests and snapshotted for queued work. Deep Research uses mode-managed model/reasoning controls
where its UI does not expose ordinary thinking levels. Missing required controls return an error
rather than silently selecting a lower setting.

To create research chats inside a ChatGPT project, open that project in ChatGPT and copy its URL
(`https://chatgpt.com/g/g-p-.../project`) into `chatgpt_project_url`. This is machine-wide for this
server's data directory, not an OpenCode project setting. `null` creates ordinary chats. New threads
retain the configured destination; changing it does not move existing threads. The service checks
the project landing page before composing and fails if it redirects or cannot be verified. Project
instructions, files, and memory may influence answers. Project routing was live-verified on Windows
in a configured ChatGPT project: the completed conversation URL contained the matching project ID.

The data directory contains `state.sqlite`, `archives/<thread-id>/thread.md`,
`archives/<thread-id>/thread.json`, the research Chrome profile, and private service/browser records.
Listing and polling do not extend inactivity. Active or ambiguous work is never deleted by expiry.
These are local transcript files; this does not use ChatGPT's **Archive chat** feature.
The `research_archive` tool reads these saved local transcripts.
If saving the transcript fails, deletion is blocked. If Chrome/login is unavailable, deletion stays pending.
Cleanup failures retry with persistent per-thread exponential backoff: 1, 2, 4, 8, 16, 32 minutes,
then at most once per hour. The retry schedule survives daemon restarts. A failed remote cleanup
stops that cleanup batch; successful deletions clear the retry counter. Existing local transcripts
are verified before retries. These retries do not send research prompts.

## Diagnostics and recovery

```sh
web-research doctor
web-research status
web-research browser-check
web-research browser-inspect
web-research login
web-research shutdown
```

- `browser-check` verifies launch/reconnect on a blank tab and checks `navigator.webdriver`.
- If the agent is missing, confirm the installed Markdown file and reload the correct project.
  If tools report a missing executable, rerun the installer with an absolute path to the built binary.
- If the UI reports `research_mode_unavailable`, `model_unavailable`, or `reasoning_unavailable`,
  inspect the selected account's controls and shared configuration. ChatGPT UI changes may require
  an adapter update; the service does not silently substitute another research mode.
- `browser-inspect` prints composer readiness and visible control labels, not transcripts.
- Stop the daemon before replacing its executable on Windows. Chrome remains open after shutdown.
- On `needs_login`, sign in in the research profile. On `needs_attention` or
  `submission_unknown`, fix the browser issue and ask the research agent to reconcile the existing
  request. Reconciliation only observes; it never automatically resends.
- Ambiguous in-flight work pauses dispatch of new requests until reconciled. A response deadline
  marks the request `timed_out`, preserving partial output and continuing observation without a retry.
- `research_cancel` can cancel queued work or request generation stop. It cannot undo a sent prompt.
- Do not manually send messages or navigate an active owned research tab while a request is running.

ChatGPT's UI changes independently of this project. See [validation status](docs/VALIDATION.md)
for what has been tested, including account-specific and optional-mode coverage.
The [agent-driven test log](docs/AGENT-LIVE-TEST.md) records the actual agent workflow, prompt review,
and any failures encountered during live integration testing.

## Development checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run check
npm test
npm run build
```

Optional Chrome DOM integration test (no account prompt):

```sh
cargo test -p research-chatgpt --test browser_fixture -- --ignored
```

The opt-in `scripts/live-smoke.mjs` test uses real account messages, normal pacing, and saves its
request IDs under `target/`. It is not part of automatic tests. On Windows, run
`$env:WEB_RESEARCH_LIVE='1'` before `node scripts/live-smoke.mjs run`; on macOS use
`WEB_RESEARCH_LIVE=1 node scripts/live-smoke.mjs run`. The `retire` action saves a local transcript and deletes
only the recorded smoke-test conversation; `archive` reads its resulting local status.

## Layout and provenance

`research-core` defines shared types/policies; `research-store` owns SQLite and archives;
`research-browser` owns Chrome/CDP; `research-chatgpt` contains UI-specific scripts and response
matching; `research-server` owns scheduling/API/lifecycle; `research-app` supplies diagnostics and bootstrap.

Inspired by [doggy8088/ask-bridge](https://github.com/doggy8088/ask-bridge), rebuilt as focused Rust
modules with an OpenCode agent integration. The initial implementation is independently written;
ask-bridge is the behavioral reference. Design decisions are in [PLAN.md](PLAN.md).

## License

[MIT](LICENSE).
