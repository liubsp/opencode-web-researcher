# Validation status

Implementation target: OpenCode 2.0.8, installed Google Chrome, Windows and macOS.

## Verified locally on Windows

- Rust workspace compilation, persistence/policy tests, and local authenticated API tests.
- Chrome 153 launch and reconnect using a persistent research profile, explicit nonzero debugging
  port, and no test-automation flag. The blank-page check reports `navigator.webdriver === false`.
- Background tab creation and minimized-window operation; renderer-only focus emulation is used
  to prevent paused answer rendering while the OS window stays minimized.
- Real Chrome fixture: composer input/recovery, Send interaction, completion controls, Markdown/code
  and citation extraction without submitting an account message.
- OpenCode project-local plugin loads as active; its installed agent is discovered. Live agent
  registry showed research permission denied for built-in agents and allowed for the research agent
  (then named `web-research`, now renamed to `web-researcher`).
- Unit tests cover request-context filtering, direct invocation denial, Code Mode exclusion options,
  and trusted project/session scoping. They do not claim to sandbox same-user shell processes.
- TypeScript type checks, unit tests, and plugin compilation.
- Global installation from the PowerShell script completes, restarts the daemon, and cleans up
  staging files. A separate directory discovers one active plugin and the inherited-model agent;
  the build agent remains denied direct research access. A global-agent request reached ChatGPT.
- Idle lifecycle tested with isolated real-Chrome profiles: browser activity resets expiry,
  expiry closes the owned browser, and subsequent access relaunches minimized Chrome and opens
  the requested ChatGPT URL after the old target is gone. A separate daemon test verified idle
  closure without an API request. No account prompts are sent by these lifecycle tests.
- Logged-in Search request with account-default model and verified Extra High selection. Captured
  the complete answer, Markdown, and source links while minimized. Observation-only reconciliation
  recovered the original request without resending; exactly one prompt was consumed.
- Chrome `browser-check` reports `window_state: minimized` and `webdriver: false` after background
  tab creation and reconnect. Both opt-in real-Chrome DOM fixtures pass, including High fallback
  when Extra High is unavailable and preserving the Search pill during composer input.
- Smoke conversation deleted through ChatGPT's UI; its explicit deleted-conversation notice was
  verified on reopening the saved URL. Local Markdown and JSON archives were verified afterward.
- Actual `web-researcher` installed in this repository completed two Search prompts in one thread,
  with Extra High selected for each and eight prompts remaining. Verified stored requests against
  the OpenCode tool trace, normal pacing, exact prompt texts, and complete responses with citations.
  This supervised test required two UI fixes and observation-only recovery for its initial request;
  the follow-up completed without another error. See [prompt review and test log](AGENT-LIVE-TEST.md).

Project routing was additionally live-verified on Windows after migrating storage to
`%LOCALAPPDATA%\web-research-opencode`. The agent submitted one normal-chat request, with no forced
Search selection and no final period. It completed with Extra High and no error. Live DOM inspection
confirmed the conversation URL used the configured project's `/g/g-p-…/c/…` route, and
the conversation appeared under that project in the sidebar. The migrated login remained valid.

## In progress

The Windows bootstrap installer was exercised from a local source checkout: release build, npm
pack/install, stable per-user paths, project setup, daemon shutdown and restart all completed.
The installed `web-research-server` health check and global-plugin reload passed. The remote-download
one-liner and macOS bootstrap still need separate verification.

- Optional Deep Research end-to-end flow, including clarification and long report completion.
- Windows/macOS CI build, test, and package jobs passed for the earlier CI correction. macOS live
  browser behavior remains unverified; Windows browser checks do not establish macOS behavior.

## Important semantics

No automatic resend occurs after persisted submission intent. Ambiguous requests retain their
budget reservation and require observation-only reconciliation. A timeout is not proof that
generation stopped and is not eligible for deletion. Only terminal work may be archived/deleted.

Expired chats and capped chats are archived locally before remote deletion; cleanup retries must
verify existing archive bytes. Missing or changed UI controls leave deletion pending instead of
guessing at another conversation.
