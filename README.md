# web-research-opencode

Give your OpenCode agent a researcher that uses ChatGPT in real Chrome.

Ask a question, let it work in the background, and get a summary with source links.
Chrome stays minimized, your login is remembered, and follow-up questions stay in the same chat.
Normal requests let ChatGPT decide when to search. Deep Research is available when you ask for it.

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

Then run the installed server's `login` command to sign in to its separate Chrome profile,
and reload your OpenCode project. See [setup](docs/INSTALLATION.md) for the exact paths,
updates, and what the installer changes in your repo.

## Use it

Ask your usual OpenCode agent:

> use web-researcher to compare these libraries using official sources, then check the weakest claim with a follow-up

For a longer investigation, say **“use web-researcher with Deep Research”**.
The researcher uses Astra Low in OpenCode; ChatGPT's model is configured separately.

Each chat allows up to 10 prompts. Messages are paced at 40 words/minute plus 15 seconds,
so give it time. After 24 hours of inactivity or the tenth response, the service saves a
local transcript before deleting the ChatGPT conversation.

## A little more detail

- [Install, update, or remove it](docs/INSTALLATION.md)
- [Models, ChatGPT projects, and saved transcripts](docs/CONFIGURATION.md)
- [Usage and troubleshooting](docs/USAGE.md)
- [How it works and how to contribute](docs/DEVELOPMENT.md)
- [What's been tested](docs/VALIDATION.md)

Inspired by [ask-bridge](https://github.com/doggy8088/ask-bridge). [MIT licensed](LICENSE).
