# Validation status

Implementation target: OpenCode 2.0.8, installed Google Chrome, Windows and macOS.

## Verified locally on Windows

- Rust workspace compilation, persistence/policy tests, and local authenticated API tests.
- Global research-slot tests cover separate database connections and projects, active-job recovery
  priority, database rejection of a second active job, full pacing after queue wait/cancellation,
  timeout/ambiguity blocking, monotonic timing, and conservative restart pacing.
- Chrome 153 launch and reconnect using a persistent research profile, explicit nonzero debugging
  port, and no test-automation flag. The blank-page check reports `navigator.webdriver === false`.
- Background tab creation and minimized-window operation; renderer-only focus emulation is used
  to prevent paused answer rendering while the OS window stays minimized.
- Real Chrome fixture: composer input/recovery, Send interaction, completion controls, Markdown/code
  and citation extraction without submitting an account message.
- Deletion-menu fixture covers a current-chat header with no sidebar history entry, rejects a
  different target URL, and leaves unrelated menus untouched.
- Installed-daemon cleanup recovered four blocked project-chat deletions. Header lookup works
  when sidebar history omits the chat; toast-free redirects are verified by reopening the exact
  saved URL for an explicit deleted-conversation notice. All four retained local Markdown archives.
  Replacement cleanup tab IDs persist across retries. One-chat-per-pass spacing remains enabled.
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
- Read-only import live-tested against a user-supplied project chat: two messages and about 45,000
  Markdown characters captured, with saved retrieval spanning three pages. A two-entry batch using
  the project URL and bare conversation ID returned identical message bodies, without creating
  managed research jobs. The source wasn't enrolled in deletion. Unit/API tests cover retry
  identity, scope isolation, Unicode pagination, partial batch results, durability, and retention.
- Full-transcript file access verified for both imports and managed threads. The installed plugin discovers
  OpenCode's temporary directory and publishes Markdown copies there; a parent-session read succeeded
  without an external-directory approval. Deleting a temporary copy and retrieving again regenerated
  identical content. Existing generated JSON exports were removed after Markdown verification.
- Transcript handoffs use plain absolute paths, not `file://` Markdown links. File access testing
  does not establish UI clickability; the agent leaves user-facing formatting to its parent.
- Logged-in Search request with account-default model and verified Extra High selection. Captured
  the complete answer, Markdown, and source links while minimized. Observation-only reconciliation
  recovered the original request without resending; exactly one prompt was consumed.
- Chrome `browser-check` reports `window_state: minimized` and `webdriver: false` after background
  tab creation and reconnect. Opt-in real-Chrome DOM fixtures pass, including High fallback
  when Extra High is unavailable and preserving the Search pill during composer input.
- Smoke conversation deleted through ChatGPT's UI; its explicit deleted-conversation notice was
  verified on reopening the saved URL. Local archives were verified afterward; exports are now Markdown-only.
- Actual `web-researcher` installed in this repository completed two Search prompts in one thread,
  with Extra High selected for each and eight prompts remaining. Verified stored requests against
  the OpenCode tool trace, normal pacing, exact prompt texts, and complete responses with citations.
  This supervised test required two UI fixes and observation-only recovery for its initial request;
  the follow-up completed without another error. See [prompt review and test log](AGENT-LIVE-TEST.md).

Project routing was additionally live-verified on Windows after migrating storage to
the shared application data directory. The agent submitted one normal-chat request, with no forced
Search selection and no final period. It completed with Extra High and no error. Live DOM inspection
confirmed the conversation URL used the configured project's `/g/g-p-…/c/…` route, and
the conversation appeared under that project in the sidebar. The migrated login remained valid.

### Reliability audit and installed recovery

- All 16 isolated Chrome fixtures pass, including multiline/zero-width text, mixed turn markup,
  literal whitespace, accessible Stop controls, scoped model selection, and applied mode checks.
  Persistence tests cover cancellation-preserving writes and single-use draft-cleanup receipts.
- Browser tests cover delayed tab-close acknowledgements and a target disappearing during window
  lookup. Closure waits for the exact target to leave the list; minimization still rejects lookup
  failures for existing targets and unrelated CDP errors. Tab closure is not chat-deletion evidence.
- Existing real research and its follow-up completed through OpenCode tools with full response
  pagination. After deployment, the original saved answer remained retrievable in two pages;
  recovery and verification did not resend submitted prompts or revive the cancelled smoke test.
- Isolated CLI lifecycle checks cover graceful activation, retained unsent request identity,
  idempotent activation, and preferred-executable bootstrap. The Windows installer also passed
  with a locked legacy executable and an application directory separate from its data home.
- Remote source download, compilation, and isolated runtime activation passed with a full commit
  reference and nested Windows TEMP directory after shortening the installer's build staging layout.
- Installed binary identity and immutable plugin revision were confirmed in the running system.
  OpenCode and its agents were not restarted. A loaded read-only tool succeeded and rejected a
  non-research agent; an isolated test through the actual OpenCode kernel confirmed interruption
  closes the underlying HTTP wait after one batch, with zero submissions.
- Registered plugin revisions survive npm package replacement; legacy revision paths are retained.
  The pinned Promise adapter's missing cancellation forwarding is bridged at the native Effect boundary.
- Two 90-second read-only production observations recorded no window-state transitions, restoration,
  or tab creation/deletion. This does not establish the cause or absence of intermittent flicker.
  Chrome was preserved during server activation and later closed under its normal inactivity policy.
- Formatting, Clippy, Rust workspace tests, TypeScript checks/tests, release/plugin builds, and
  package/privacy inspection passed locally. Local Windows checks do not establish macOS live
  browser compatibility.

### Cleanup compatibility and operational health

- Current live project-row menus and confirmation dialogs were inspected without confirming deletion.
  Header More opens Plugins, not Delete; older chats can be absent from truncated sidebar history.
  Cleanup now falls back to the exact chat row in its owned project, with bounded hydration waits.
- Regression fixtures cover friendly project-slug changes, immutable chat/project identity, accessible
  row controls, trigger-bound menus, the Delete chat dialog button, and rejection of unrelated or
  ambiguous controls. Generic redirects, access errors, and unbound deletion toasts are not proof.
- Health and ordinary retrieval results surface failed creation, ambiguous requests, and blocked
  overdue cleanup. API tests cover project scoping, immediate explicit cleanup retries, and degraded
  configuration without breaking protocol discovery. Stop retry spacing survives observation yields.
- Cleanup retries reuse owned home/project redirects instead of accumulating tabs; unrelated
  conversations and project destinations are rejected. The PowerShell installer passed isolated
  locked-launcher cutover and separate-data-home tests using .NET SHA-256 without Get-FileHash.
- Deletion verification observes the exact UI-generated request and its successful acknowledgement,
  without issuing ChatGPT account API calls. Receipts contain only the owned URL, evidence type, and
  timestamp, and survive a restart between remote deletion and local retirement. Tests reject other
  chats, non-deletion updates, redirects, failed HTTP responses, and contradictory acknowledgements.
- Two disposable live requests completed with one prompt each. An independent observer captured the
  first UI deletion's successful acknowledgement; its diagnostic record was reconciled from that
  evidence after a backup. The final installed retest retired automatically with a durable UI-response
  receipt after recovering from a transient menu-readiness failure. No original research was resent.
- Previously visible overdue chats are absent from the project. Three historical records remain
  explicitly unconfirmed because reopening them now yields access denial and no deletion receipt was
  captured at the time. Their local archives are retained; disappearance is not retroactive proof.
- Final installed server and loaded plugin identities were verified through the existing OpenCode
  service without restarting OpenCode or agents. All original saved requests and nine within-retention
  thread records were unchanged; the project retained its seven originally visible retained chats,
  with no overdue or untracked rows. Operation health exposes the three unresolved cleanup records.

## In progress

The published Windows bootstrap script and its remote source download were exercised in an isolated
installation, including compilation and runtime verification. macOS bootstrap still needs separate
verification; shell syntax checks are not evidence of macOS execution.

- Optional Deep Research end-to-end flow, including clarification and long report completion.
- Windows/macOS CI covers builds, tests, isolated Chrome fixtures, runtime activation, and packages.
  macOS fixtures start isolated normal Chrome directly because hosted-runner LaunchServices stalls.
  They verify CDP/DOM compatibility, not nonactivating desktop launch or live account behavior.

## Important semantics

No automatic resend occurs after persisted submission intent. Ambiguous requests retain their
budget reservation and require observation-only reconciliation. A timeout is not proof that
generation stopped and is not eligible for deletion. Only terminal work may be archived/deleted.

Expired chats and capped chats are archived locally before remote deletion; cleanup retries must
verify existing archive bytes. Missing or changed UI controls leave deletion pending instead of
guessing at another conversation.
