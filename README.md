# 🦀 OpenCode Web Researcher

[![Rust](https://img.shields.io/badge/Built_with-Rust-000000?logo=rust)](https://www.rust-lang.org/)
[![OpenCode V2](https://img.shields.io/badge/Works_with-OpenCode_V2-18181B)](https://opencode.ai/)

Give your OpenCode agent a researcher that uses ChatGPT in real Chrome.

It asks questions, explores options, follows up as it learns, and brings sourced findings back to
your coding agent. It uses your ChatGPT account—no OpenAI API key or browser extension needed.

- **Conversational research**, with Deep Research when you explicitly ask for it.
- **Read existing ChatGPT chats** or revisit research saved locally.
- **Chrome runs minimized**, with a separate persistent login.
- **Install per repo or globally**, sharing one login and research queue.

## Install

Requires **OpenCode V2, Google Chrome, Node.js 24+, and Rust stable**. The installer builds from
source. Tested with OpenCode 2.0.8 on Windows; macOS validation is ongoing.

Run from the repo where you want to use it:

**Windows · PowerShell**

```powershell
& ([scriptblock]::Create((Invoke-WebRequest -UseBasicParsing 'https://raw.githubusercontent.com/liubsp/opencode-web-researcher/main/scripts/install.ps1').Content)) -Project (Get-Location).Path
```

**macOS**

```sh
curl -fsSL https://raw.githubusercontent.com/liubsp/opencode-web-researcher/main/scripts/install.sh | bash -s -- "$PWD"
```

For all projects, replace `-Project (Get-Location).Path` with `-Global` on Windows, or `"$PWD"`
with `--global` on macOS. See [installation](docs/INSTALLATION.md) for setup details, updates,
and removal.

### Sign in

**Windows · PowerShell**

```powershell
& "$env:LOCALAPPDATA\opencode-web-researcher\app\bin\opencode-web-researcher.exe" login
```

**macOS**

```sh
"$HOME/Library/Application Support/opencode-web-researcher/app/bin/opencode-web-researcher" login
```

Sign in to ChatGPT in the window that opens, then reload your OpenCode project. This profile is
separate from your everyday Chrome login. The background server starts automatically.
Upgrading an older installation? Use the server path printed by the installer; it reuses existing data.

## Use it

Ask your usual OpenCode agent:

> use web-researcher to compare these libraries and check which fits our project

> use web-researcher with Deep Research to investigate this

> use web-researcher to read these ChatGPT chats and summarize where they disagree: `<chat-url-1>`, `<chat-url-2>`

> use web-researcher to find our earlier research about Chrome profiles, without sending a new prompt

You can supply context, logs, and follow-up questions as the investigation develops. Existing chats
are read without sending messages or deleting them. See [usage](docs/USAGE.md) for more examples
and troubleshooting.

## What to expect

Research takes time: pre-send waits use **40 words/minute plus a 15–45-second pause**, and work is queued
across projects. Each research chat allows up to **10 prompts**.

Captured exchanges are saved locally. Chats created by the researcher are automatically deleted
from ChatGPT after **a per-chat random 1–7 days of inactivity or the tenth response**; local history is kept for
**30 days** by default. Supplied source chats aren't enrolled in this cleanup.

Defaults work without configuration. To choose a ChatGPT model or project, adjust timing, or change
retention, see [settings and saved data](docs/CONFIGURATION.md).

## Documentation

- [Installation, updates, and removal](docs/INSTALLATION.md)
- [Usage and troubleshooting](docs/USAGE.md)
- [Settings and saved data](docs/CONFIGURATION.md)
- [Development and architecture](docs/DEVELOPMENT.md)
- [Validation status](docs/VALIDATION.md)

Inspired by [ask-bridge](https://github.com/doggy8088/ask-bridge). [MIT licensed](LICENSE).
