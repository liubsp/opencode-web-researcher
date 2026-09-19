# Setup

Install once on your machine, then opt in each repository where you want the researcher.
The server and Chrome login are shared; OpenCode's plugin registration is per repo.

## The quick route

Use the [README commands](../README.md#install) from the target repository. They download and build
the source, install the app at a stable path, and register it in that repo. You'll need Rust stable,
Node.js 24/npm, OpenCode V2, and installed Google Chrome. macOS also needs Xcode command-line tools.
OpenCode 2.0.8 is our tested version, not a verified minimum. The plugin currently pins
`@opencode/plugin` to 2.0.8; compatibility with other V2 app releases hasn't been checked yet.

The shared data directory is:

- Windows: `%LOCALAPPDATA%\web-research-opencode`
- macOS: `~/Library/Application Support/web-research-opencode`

Inside it, the server lives at `app/bin/web-research-server.exe` on Windows or
`app/bin/web-research-server` on macOS. The plugin lives at
`app/runtime/node_modules/web-research-opencode`.

Sign in once:

```powershell
& "$env:LOCALAPPDATA\web-research-opencode\app\bin\web-research-server.exe" login
```

```sh
"$HOME/Library/Application Support/web-research-opencode/app/bin/web-research-server" login
```

This opens a separate Chrome profile. Your everyday Chrome login isn't inherited.
Reload the target OpenCode location after installation, then ask it to use `web-researcher`.

## Another repo, or an update

Run the same installer from another repo to opt it in. All repos reference the same stable app
paths, so they don't depend on where you downloaded or built the source.
If switching from a checkout-based installation, remove its old plugin entry before installing
the stable copy. Otherwise the two different package paths can register the plugin twice.

Rerunning the installer updates the shared app. Omit `-Project` in PowerShell or the project argument
in the shell command to update only the app. The default revision is `main`; pin a revision with
`-Ref <commit-or-tag>` on Windows or `WEB_RESEARCH_REF` in the macOS script's environment.
For example, pass that variable to `bash`, not just to the `curl` side of the pipeline.

The updater builds first, then stops the running daemon, replaces the app, and restarts it if it
was running. Otherwise the next research call starts it. Login and research data stay in place.
Prefer updating while research is idle. Interrupted submissions keep their recorded state;
don't resend a question just because an update interrupted the wait.

Reload open OpenCode locations to pick up plugin changes. Project agent files are separate:
rerun project setup to install them, and review differences if it reports an existing-file conflict.
The installer doesn't overwrite customized instructions. Updates aren't transactional rollbacks;
if copying or npm installation fails, fix the error and rerun. Concurrent installs are serialized.

## What changes in your repo?

- A plugin entry is added to `opencode.jsonc`, or your existing `opencode.json`. Settings and comments
  are preserved. If both files exist, setup asks you to resolve that ambiguity.
- `.opencode/agents/web-researcher.md` is created. A differing existing file is left alone.
- Those files appear in Git status. Nothing is committed or pushed. The config includes absolute
  local paths, so teammates should install with their own paths.
- Application source, dependency manifests, lockfiles, remotes, and Git hooks aren't changed.
  npm dependencies are installed with the plugin, not into your application.

For a local-only setup, put the untracked config and agent file in `.git/info/exclude`. This doesn't
hide edits to an already tracked config file; review those changes before committing.

Opted-in repos share the daemon, queue, ChatGPT login, and machine settings. Research in one repo
can delay another. Prompts go to ChatGPT and use that account's limits. The researcher has no file
editing or shell permissions, although your parent coding agent keeps its usual permissions.

## Build from a checkout instead

From the plugin checkout:

```sh
cargo build --release --locked -p research-app
npm ci
npm run build
```

Then register it in a target repo:

```powershell
node packages/opencode-plugin/dist/install.js --project "C:\projects\my-app" --binary "C:\tools\web-research-opencode\target\release\web-research.exe"
```

```sh
node packages/opencode-plugin/dist/install.js --project /path/to/my-app --binary /path/to/web-research-opencode/target/release/web-research
```

Source builds use the executable name `web-research`; bootstrap installations name it
`web-research-server`. Both accept the same commands. Debug builds also work for development.
Keep the checkout and executable at their registered paths when using this installation method.

CI packages an executable and npm tarball. To use those, install the tarball in a permanent tools
directory and run its setup command with absolute paths:

```sh
npm install /path/to/web-research-opencode-0.1.0.tgz
npx --no-install web-research-setup --project /path/to/my-app --binary /path/to/web-research-server
```

Prebuilt binaries don't need Rust. This package hasn't been published to npm.

## Remove it

Remove the plugin entry and `.opencode/agents/web-researcher.md` from a repo, then reload that
OpenCode location. Shared login and transcripts remain available to other opted-in repos.
The server's `shutdown` command stops the daemon; another opted-in repo can start it again.
