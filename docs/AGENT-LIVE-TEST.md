# Agent-driven integration test

Date: 2026-09-19. Environment: Windows, Chrome 153, OpenCode 2.0.8.

**Result: completed with two submitted Search messages in one thread, after two adapter fixes.**
Both responses are complete. Final budget: **8 remaining**. This was a supervised integration test:
the first attempt failed before submission and the initial sent message required observation-only
reconciliation. The substantive follow-up then completed without another error.

## Setup and scope

- Installed the plugin into this repository with `dist/install.js --project .` and the debug Rust binary.
- Reloaded the OpenCode location. Its API reported the local plugin active and discovered
  `.opencode/agents/web-researcher.md` with `web_research` permission allowed.
- Delegated the task to the actual `web-researcher` subagent. No smoke-test client submitted its prompts.
- Task: research official Chrome/CDP guidance for persistent profiles, background tabs, and minimized
  rendering, then use one substantive follow-up in the same ChatGPT thread to check a weak claim.
- Search only, ordinary pacing, two submitted prompts authorized. Keep the thread available afterward.

## First attempt: failure before submission

The agent called `research_start`, then four `research_wait` calls with `seconds: 60`. The job moved
through queued, pacing, and preparing before failing with `research_mode_unavailable`. Its
`submitted_at` remained null and the prompt reservation was refunded (10 remaining).

Exact proposed text (not sent to ChatGPT):

> im building rust background service controlling real Chrome with CDP. can you research official Chrome/CDP docs for persistent separate profiles, creating tabs in background, and keeping a minimized tab rendering active without bringing OS window forward? use Search, not Deep Research. need exact flags and protocol methods, what actually guaranteed vs not, and tradeoffs for normal headed Chrome vs headless. check current docs and any version limits, give official source links and dates pls. especially separate browser login data surviving restarts and screenshots or page updates while window minimized.

### Prompt and agent review

- Understandable casual English; technical names retained; no role-play or claim to be human.
- Requests official sources and distinguishes guarantees from assumptions.
- Too broad for this focused check: the headed/headless comparison and screenshots expand the task.
  The continuation asks for a shorter, more focused question.
- Correct failure handling: patient waits, no duplicate submission, no replacement thread, no invented
  findings, and clear reporting of the error and unused budget.

### Adapter correction

The generic `More` button selector selected the sidebar menu instead of the composer tool menu.
`open_tools` now prefers `composer-plus-btn` and restricts fallback candidates to the composer form.
Verified the corrected action against the actual tab and added a sidebar-More regression fixture.
Both real-Chrome fixture tests passed after rebuilding.

## Continuation: submitted message, confirmation recovery

The agent used `research_send` with a new request key in the existing thread, because the original
request was terminal and confirmed unsent.

Exact submitted initial question:

> im building Rust Chrome background service. can you search official docs on separate persistent Chrome profiles with --user-data-dir, Target.createTarget background tabs, and Emulation.setFocusEmulationEnabled for keeping minimized rendering active without OS window activation? need exact guarantees vs assumptions, version limits, and official source links with dates. does focus emulation actually keep screenshots and animation updating while minimized?

Review: 56 whitespace-delimited words, focused, casually phrased, and preserves exact protocol method names. Asking whether
focus emulation actually keeps rendering active is appropriate; it does not assume that it does.

After three 60-second waits, the tool returned `submission_unknown` with a submission timestamp and
9 remaining prompts. The agent correctly stopped without resending. Live DOM inspection confirmed
the exact message was present in ChatGPT. The adapter had included the longer user-message bubble's
“Show more / Show less” button labels in its text comparison.

The extraction now excludes buttons as well as the Search pill; a regression fixture verifies the
message text remains exact. Both real-Chrome fixtures passed. The agent was explicitly told the
issue was fixed and asked to reconcile this request by observation only, then send one substantive
follow-up after the answer completes. Reconciliation succeeded without resending.

## Completed follow-up and prompt review

Exact submitted follow-up (60 whitespace-delimited words):

> ok weakest part seems minimized rendering without OS activation. can you verify that against official docs and Chromium source, with exact short quotes and pinned source links? does IncrementCapturerCount cover minimized Windows window specifically, or only background/occluded content? check whether focus emulation activates native window anywhere. separate documented promise, implementation evidence, and inference; correct your earlier yes if too strong.

This was a useful evidence-driven follow-up rather than a repetition of the original question.
It challenged the strongest unsupported inference, asked for pinned evidence, and allowed correction.
It stayed casual while retaining exact technical terms. ChatGPT explicitly acknowledged that its
earlier “yes” was too categorical and narrowed its conclusion.

The agent's final report distinguished ChatGPT-supplied sources from independently checked sources,
noted mixed source revisions, reported uncertainty, and preserved IDs/budget. It did not claim to
have independently visited the cited pages. Its summary was detailed; a shorter parent-facing
summary would normally suffice outside a test requiring full evidence and prompt review.

## Persisted-record and tool-trace verification

Checked the daemon's stored requests against the OpenCode session's actual tool-call records:

- Agent identity: `web-researcher`; calls were `research_start`, `research_send`, `research_wait`,
  and `research_reconcile`. The observed trace contains one start, two sends, one reconcile, and
  21 waits. The failed start never sent a ChatGPT message.
- Both successful requests selected **Search + Extra High**, with the account-default `Latest`
  model label. Both have `state: completed`, `response.complete: true`, and no final error.
- Initial sent question: minimum composition delay 99 seconds; actual creation-to-submission
  interval 100 seconds. Response extraction retained 25 citation entries.
- Follow-up: minimum composition delay 105 seconds; actual creation-to-submission interval
  107 seconds. Response extraction retained 14 citation entries (not necessarily distinct sources).
- One thread, two counted prompts, eight remaining. The thread is still active and was not retired
  by the test; normal inactivity cleanup still applies.

## Research outcome

The responses supported dedicated non-default `--user-data-dir` storage and background target
creation, while the follow-up narrowed the minimized-rendering claim: focus emulation's public CDP
contract is weaker than a guarantee of continuous minimized animations or fresh screenshots.
ChatGPT supplied implementation evidence, but its cited trace mixed moving HEAD and pinned revisions.
Treat the behavior as something to validate against the deployed Chrome build.

Representative links supplied by ChatGPT (not independently verified in this test):

- https://chromium.googlesource.com/chromium/src/+/HEAD/docs/user_data_dir.md
- https://developer.chrome.com/blog/remote-debugging-port
- https://chromedevtools.github.io/devtools-protocol/1-3/Target/
- https://chromium.googlesource.com/chromium/src.git/+/HEAD/content/browser/devtools/protocol/emulation_handler.cc
- https://chromium.googlesource.com/chromium/src/+show/refs/heads/main/docs/windows_native_window_occlusion_tracking.md

This validates the agent-to-tool-to-daemon-to-ChatGPT workflow, multi-step conversation reuse,
patient waiting, recovery, prompt accounting, and a useful source-verification follow-up. It does
not establish every Chrome implementation claim, validate macOS, or exercise Deep Research.

## Project-scoped normal-chat verification

After the default changed from forced Search to automatic chat mode, the agent completed a new
one-message test in a configured ChatGPT project.

> can you check why Chrome remote debugging needs a dedicated --user-data-dir since Chrome 136? brief answer pls, with one official source link

Persisted selection was `auto` with Extra High; the request completed without error. Live DOM
inspection independently confirmed the resulting URL contained the configured project's ID and
the conversation appeared beneath the configured project in the sidebar. No Search pill was present. The answer
included the official Chrome remote-debugging article. One prompt was consumed; the thread remains
active. This also verified that login survived moving the shared data to the new default directory.
