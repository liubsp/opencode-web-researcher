# 🦀 Web Research for OpenCode

[![Rust](https://img.shields.io/badge/Built_with-Rust-000000?logo=rust)](https://www.rust-lang.org/)
[![OpenCode V2](https://img.shields.io/badge/Works_with-OpenCode_V2-18181B)](https://opencode.ai/)

Give your OpenCode agent a researcher that uses ChatGPT in real Chrome.

Delegate an investigation without leaving your coding session. The researcher asks ChatGPT,
reads the answers, follows up on gaps, and brings the findings back to your coding agent.
The full captured exchanges are kept locally, not just the final report.

## What you get

- **Multi-step research.** Compare options, investigate a technical question, or challenge a claim.
  Follow-ups stay in the same conversation, and you can resume earlier work by thread ID.
- **Your ChatGPT account, in real Chrome.** Reuses a separate persistent login profile; no OpenAI
  API key or browser extension needed. Chrome stays minimized while research runs.
- **Normal chat or Deep Research.** ChatGPT decides when to search in normal mode. Deep Research
  starts only when you explicitly ask for it.
- **Answers you can revisit.** Captures response text, Markdown, and source links when provided.
  Saves each prompt before sending and each completed exchange locally, even if the remote chat
  is later deleted. The agent distinguishes cited material from sources it independently checked.
- **A dedicated OpenCode agent.** Only `web-researcher` can use the research tools; your coding
  agent delegates to it. It inherits your OpenCode session's model, with separate ChatGPT model/thinking settings.
- **One shared setup, your choice of repos.** Opt in each repo separately; they share the background
  server, login, settings, and queue. You can route new chats into a configurable ChatGPT project.
- **Patient, bounded conversations.** Messages are paced at 40 words/minute plus 15 seconds, with
  up to 10 prompts per chat. Recovers interrupted requests without automatically sending duplicate messages.
- **Local history, automatic cleanup.** Keeps local transcripts and deletes managed ChatGPT chats
  after 24 hours of inactivity or the tenth response, once their final local copy is verified.

## Install

You'll need OpenCode V2, Google Chrome, Node.js 24+, and Rust stable.
We've tested with OpenCode 2.0.8.
The installer builds from source and keeps the app in your user-data directory—no permanent
Git checkout needed. Windows has been live-tested; macOS support is still being validated.

Run this from the repo where you want to use it:

**Windows · PowerShell**

```powershell
& ([scriptblock]::Create((Invoke-WebRequest -UseBasicParsing 'https://raw.githubusercontent.com/liubsp/web-research-opencode/main/scripts/install.ps1').Content)) -Project (Get-Location).Path
```

**macOS**

```sh
curl -fsSL https://raw.githubusercontent.com/liubsp/web-research-opencode/main/scripts/install.sh | bash -s -- "$PWD"
```

Setup adds a plugin entry to your repo's `opencode.json(c)` and creates
`.opencode/agents/web-researcher.md`. It doesn't change your application code or dependencies,
and doesn't commit anything. The config contains local absolute paths, so review it before committing.
If switching from a checkout-based installation, remove its old plugin entry first.

### Or install for every project

Use `-Global` instead of `-Project ...` on Windows:

```powershell
& ([scriptblock]::Create((Invoke-WebRequest -UseBasicParsing 'https://raw.githubusercontent.com/liubsp/web-research-opencode/main/scripts/install.ps1').Content)) -Global
```

On macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/liubsp/web-research-opencode/main/scripts/install.sh | bash -s -- --global
```

This registers the plugin in `~/.config/opencode/opencode.json(c)` and the agent in
`~/.config/opencode/agents/web-researcher.md` (or under `XDG_CONFIG_HOME` when set).
It makes the researcher available across projects without adding files to each repo. Tool access
still belongs only to `web-researcher`, and results remain project-scoped. Remove old per-repo
registrations when switching to global installation, then reload your OpenCode locations.

## Sign in

**Windows · PowerShell**

```powershell
& "$env:LOCALAPPDATA\web-research-opencode\app\bin\web-research-server.exe" login
```

**macOS**

```sh
"$HOME/Library/Application Support/web-research-opencode/app/bin/web-research-server" login
```

Sign in to ChatGPT in the window that opens, then reload your OpenCode project.
This is a separate Chrome profile, so you only need to sign in once; your everyday Chrome login
isn't inherited. Normal research keeps this window minimized. The server starts automatically.

## Configure

The installer creates a shared `config.json`:

- **Windows:** `%LOCALAPPDATA%\web-research-opencode\config.json`
- **macOS:** `~/Library/Application Support/web-research-opencode/config.json`

You can leave the defaults alone, or edit the file:

```json
{
  "model": "default",
  "chatgpt_project_url": null,
  "reasoning_preferences": ["Extra High", "High"],
  "words_per_minute": 40,
  "fixed_pause_seconds": 15,
  "remote_chat_inactivity_hours": 24,
  "local_transcript_retention_days": 30,
  "search_timeout_seconds": 900,
  "deep_research_timeout_seconds": 1800,
  "chrome_path": null
}
```

- **Model:** `default` keeps your ChatGPT account's choice. To choose another, use its label in
  ChatGPT, not an API model ID. The OpenCode researcher inherits its parent session's model unless
  you configure an agent-specific override in OpenCode.
- **Thinking:** tries Extra High, then High if unavailable. If neither exists, it reports an error.
  Deep Research uses its own mode-managed controls.
- **ChatGPT project:** to put new chats in a project, open it in ChatGPT and paste its full
  `https://chatgpt.com/g/g-p-…/project` URL into `chatgpt_project_url`. Leave it `null` for ordinary
  chats. Existing threads stay where they were created. Project instructions and files can affect answers.
- **Timing:** normal responses have a 15-minute deadline; Deep Research has 30 minutes. A timeout
  preserves captured results and keeps observing—it doesn't resend the question.
- **Chrome:** leave `chrome_path` as `null` for automatic discovery, or supply its executable path.

Settings apply to new requests without a restart; queued requests keep their original settings.
All opted-in repos share these settings, the ChatGPT login, and one queue. Work in one repo may
delay another. Prompts go to your ChatGPT account and use its limits; no OpenAI API key is needed.

## Use it

Ask your usual OpenCode agent:

> use web-researcher to compare these libraries using official sources, then check the weakest claim with a follow-up

For a longer investigation, say **“use web-researcher with Deep Research”**.
ChatGPT's model is configured separately from your OpenCode model.
Deep Research only starts when explicitly requested, not just because a question is complex.
To continue earlier work, give the agent the thread ID from its report and ask it to resume.

Each chat allows up to 10 prompts. Messages are paced at 40 words/minute plus 15 seconds,
so give it time. Each prompt is saved locally before sending, and each completed exchange is
saved as Markdown and JSON. After 24 hours of inactivity or the tenth response, the service
verifies a final local transcript before deleting the ChatGPT conversation.

Local copies live beside `config.json`, under `transcripts/<thread-id>/<request-id>/`.
Final full-chat copies live under `archives/`. These aren't ChatGPT's **Archive chat** feature.
Deleting a chat in ChatGPT doesn't remove captured local records. An answer deleted before it was
captured can't be recovered. Local transcripts and database history expire after 30 days since
the thread's last activity, once remote cleanup is confirmed. Active/unresolved work and pending
remote deletion are retained. Adjust `local_transcript_retention_days` to change this limit.

### Reuse earlier research

Ask your coding agent to delegate retrieval to the researcher:

> use web-researcher to list this repo's previous research, including retired chats, and retrieve the findings about Chrome profiles without sending anything to ChatGPT

If you have the thread ID from an earlier report:

> use web-researcher to read the saved results for thread `<thread-id>` and use them to answer this question; don't send a new prompt

The researcher uses `research_list` to find threads and `research_archive` to read stored prompts,
answers, and citations—even for a chat deleted from ChatGPT. These operations read the local
database and don't consume prompts or need Chrome. Despite its name, `research_archive` also
reads results from active threads; it doesn't invoke ChatGPT's Archive feature.

You can also open the files yourself, without OpenCode or the server:

- Open the data directory beside `config.json` (paths are in **Configure** above).
- Under `transcripts/<thread-id>/<request-id>/`, read **`exchange.md`** for the question and captured
  answer. **`exchange.json`** includes request metadata and captured citations; **`prompt.json`**
  is saved before sending, so it can exist before an answer is available.
- After cleanup, `archives/<thread-id>/thread.md` and `thread.json` contain the final whole-thread copy.

Reading old results is different from continuing the remote conversation. If the ChatGPT chat
still exists and the thread is unexpired and below its limit, ask the researcher to resume it and
send a follow-up. If it was deleted or retired, its captured results remain readable, but that
remote chat can't be continued. Keep the database as well as the transcript folders when backing
up: the agent retrieves history from the database, while the files are independently readable copies.

## Another repo, updates, and removal

Run the same install command from another repo to enable it there. Every repo points to the same
installed app, not a Git checkout.

Rerun the installer to update. It builds first, stops the shared server, replaces the app, and
restarts it if it was running. Your login and data stay in place. Update while research is idle
when possible, then reload your open OpenCode projects. If setup reports a differing agent file,
compare it with the installed package's `agents/web-researcher.md` before replacing it; customized
instructions aren't overwritten automatically.

## Uninstall

**For a global installation:** remove the plugin entry from your global
`~/.config/opencode/opencode.json(c)` and delete `~/.config/opencode/agents/web-researcher.md`,
using `XDG_CONFIG_HOME` instead of `~/.config` if configured. Reload open OpenCode locations.
Any separate per-repo registrations must be removed separately.

**From one repo:**

1. Remove the `web-research-opencode` plugin entry from `opencode.json` or `opencode.jsonc`.
   Keep any other settings and plugins in that file.
2. Delete `.opencode/agents/web-researcher.md` (keep a copy if you customized it).
3. Reload that OpenCode project. Remove any local Git exclude entries you added for these files.

Other opted-in repos keep working. Your shared login and saved transcripts aren't deleted.

**Remove the shared app from your machine:** first unregister it from every opted-in repo, then
stop the server and delete its `app` directory:

Windows PowerShell:

```powershell
& "$env:LOCALAPPDATA\web-research-opencode\app\bin\web-research-server.exe" shutdown
Remove-Item -LiteralPath "$env:LOCALAPPDATA\web-research-opencode\app" -Recurse -Force
```

macOS:

```sh
"$HOME/Library/Application Support/web-research-opencode/app/bin/web-research-server" shutdown
rm -rf "$HOME/Library/Application Support/web-research-opencode/app"
```

If the server was already stopped, `shutdown` may report that it can't connect. On Windows, if
deletion says a file is busy, let active requests finish shutting down and retry. Chrome stays
open after server shutdown; close the dedicated research Chrome window when you're finished.

These commands keep your settings, Chrome login profile, database, and transcripts. For a complete
data removal, back up any transcripts you want, close the research Chrome window, and delete the
entire `web-research-opencode` data directory shown under **Configure**. That also removes the
saved ChatGPT login. Uninstalling doesn't delete conversations from your ChatGPT account, and
automatic conversation cleanup stops when the server is removed.

## If something gets stuck

Use the same full server path as the login command above, replacing `login` with:

| Command | What it does |
| --- | --- |
| `doctor` | Show settings, paths, and Chrome discovery |
| `status` | Check whether the server is running |
| `browser-check` | Check Chrome connection and minimized operation |
| `browser-inspect` | Check ChatGPT controls and login readiness |
| `shutdown` | Stop the shared server; Chrome stays open |
| `configure` | Create default config if missing and print its path |

If the agent is missing, reload the right OpenCode project. If login is needed, run `login` again.
For `submission_unknown` or `needs_attention`, fix the browser issue and ask the researcher to
**reconcile the existing request**, rather than send it again. Don't manually navigate or send
messages in its active research tab. ChatGPT UI changes can require a plugin update.

## For developers and deeper details

- [Install, update, or remove it](docs/INSTALLATION.md)
- [Models, ChatGPT projects, and saved transcripts](docs/CONFIGURATION.md)
- [Usage and troubleshooting](docs/USAGE.md)
- [How it works and how to contribute](docs/DEVELOPMENT.md)
- [What's been tested](docs/VALIDATION.md)

Inspired by [ask-bridge](https://github.com/doggy8088/ask-bridge). [MIT licensed](LICENSE).
