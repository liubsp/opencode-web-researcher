# Setup

Install once on your machine, then register the researcher per repository or globally.
The server and Chrome login are shared across registered projects.

## Global installation

For all-project availability:

```powershell
& ([scriptblock]::Create((Invoke-WebRequest -UseBasicParsing 'https://raw.githubusercontent.com/liubsp/opencode-web-researcher/main/scripts/install.ps1').Content)) -Global
```

```sh
curl -fsSL https://raw.githubusercontent.com/liubsp/opencode-web-researcher/main/scripts/install.sh | bash -s -- --global
```

This registers the plugin in `~/.config/opencode/opencode.json(c)` and the agent in
`~/.config/opencode/agents/web-researcher.md`, or under `XDG_CONFIG_HOME` when configured.
Remove old per-repo registrations when switching to global setup, then reload OpenCode locations.

The bootstrap options are `-Global` (PowerShell) and `--global` (shell). Direct package setup also
accepts `web-research-setup --global --binary <absolute-server-path>`. Global and project options
are mutually exclusive. Global setup preserves existing settings and refuses to overwrite a
different agent definition, just like project setup.

## The quick route

Use the [README commands](../README.md#install) from the target repository. They download and build
the source, install the app at a stable path, and register it in that repo. You'll need Rust stable,
Node.js 24/npm, OpenCode V2, and installed Google Chrome. macOS also needs Xcode command-line tools.
OpenCode 2.0.8 is our tested version, not a verified minimum. The plugin currently pins
`@opencode/plugin` to 2.0.8; compatibility with other V2 app releases hasn't been checked yet.

The shared data directory is:

- Windows: `%LOCALAPPDATA%\opencode-web-researcher`
- macOS: `~/Library/Application Support/opencode-web-researcher`

Upgrades reuse an existing `web-research-opencode` data directory. Use the server path printed by the
installer for login and removal commands below. Rerun setup in each registered location to replace
the old package registration with `opencode-web-researcher`.

Inside it, the server lives at `app/bin/web-research-server.exe` on Windows or
`app/bin/web-research-server` on macOS. The plugin lives at
`app/runtime/node_modules/opencode-web-researcher`.

Sign in once:

```powershell
& "$env:LOCALAPPDATA\opencode-web-researcher\app\bin\web-research-server.exe" login
```

```sh
"$HOME/Library/Application Support/opencode-web-researcher/app/bin/web-research-server" login
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
node scripts/build-release.mjs
npm ci
npm run build
```

Then register it in a target repo:

```powershell
node packages/opencode-plugin/dist/install.js --project "C:\projects\my-app" --binary "C:\tools\opencode-web-researcher\target\release\web-research.exe"
```

```sh
node packages/opencode-plugin/dist/install.js --project /path/to/my-app --binary /path/to/opencode-web-researcher/target/release/web-research
```

Source builds use the executable name `web-research`; bootstrap installations name it
`web-research-server`. Both accept the same commands. Debug builds also work for development.
Keep the checkout and executable at their registered paths when using this installation method.

CI packages an executable and npm tarball. To use those, install the tarball in a permanent tools
directory and run its setup command with absolute paths:

```sh
npm install /path/to/opencode-web-researcher-0.1.0.tgz
npx --no-install web-research-setup --project /path/to/my-app --binary /path/to/web-research-server
```

Prebuilt binaries don't need Rust. This package hasn't been published to npm.

## Remove it

For a global installation, remove the plugin entry from `~/.config/opencode/opencode.json(c)` and
delete `~/.config/opencode/agents/web-researcher.md`, using `XDG_CONFIG_HOME` when configured.

For a per-repo installation, remove the plugin entry from `opencode.json(c)` and delete
`.opencode/agents/web-researcher.md`. Preserve other settings and any customized instructions you
want to keep. Remove local Git exclude entries you added for these files.

Reload affected OpenCode locations. Other registered repos keep working; shared login and saved
research aren't deleted. Remove any separate global or per-repo registrations as needed.

To remove the shared app, first unregister it everywhere, then stop the server and delete `app`:

```powershell
& "$env:LOCALAPPDATA\opencode-web-researcher\app\bin\web-research-server.exe" shutdown
Remove-Item -LiteralPath "$env:LOCALAPPDATA\opencode-web-researcher\app" -Recurse -Force
```

```sh
"$HOME/Library/Application Support/opencode-web-researcher/app/bin/web-research-server" shutdown
rm -rf "$HOME/Library/Application Support/opencode-web-researcher/app"
```

If already stopped, `shutdown` may report that it can't connect. On Windows, let active requests
finish shutting down before retrying a busy-file error. Chrome stays open after server shutdown;
close the dedicated research window when finished.

These commands preserve settings, login, database, and transcripts. To remove all saved data,
back up what you need, close research Chrome, and delete the entire shared data directory listed
above. Uninstalling doesn't delete conversations in ChatGPT; automatic cleanup stops with the server.
