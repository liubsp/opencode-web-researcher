# Settings and saved data

Run the server's `configure` command to create the shared `config.json` and print its path.
`doctor` shows the current paths and settings without starting research.

By default, the config file is at:

- Windows: `%LOCALAPPDATA%\opencode-web-researcher\config.json`
- macOS: `~/Library/Application Support/opencode-web-researcher/config.json`

Existing installations reuse the legacy `web-research-opencode` data directory if present, keeping
login, history, and the shared queue together. `WEB_RESEARCH_HOME` takes precedence over both defaults.

For tests or a separate server instance, set `WEB_RESEARCH_HOME`. The plugin and executable must
inherit the same value. This selects a different data directory, including its profile and queue.

```json
{
  "model": "default",
  "chatgpt_project_url": null,
  "reasoning_preferences": ["Extra High", "High"],
  "words_per_minute": 40,
  "fixed_pause_seconds": 15,
  "pause_jitter_seconds": 30,
  "chat_deletion_delay_min_hours": 24,
  "chat_deletion_delay_max_hours": 168,
  "local_transcript_retention_days": 30,
  "search_timeout_seconds": 900,
  "deep_research_timeout_seconds": 1800,
  "chrome_path": null,
  "chrome_auto_close": true,
  "chrome_idle_timeout_minutes": 30,
  "ui_action_delay_min_seconds": 2,
  "ui_action_delay_max_seconds": 5,
  "account_action_delay_min_seconds": 60,
  "account_action_delay_max_seconds": 180,
  "page_load_delay_min_seconds": 300,
  "page_load_delay_max_seconds": 600
}
```

## Imported source chats


Read-only source captures live in the separate `chat_reads` SQLite table and
`imports/<read-request-id>/<chat-index>.md`. They never become managed research threads,
consume prompt reservations, or enter remote deletion. Completed/cancelled batches expire after
`local_transcript_retention_days` since their last captured result; pending batches are retained.
Saving and retrieval are project-scoped. The researcher returns paginated Markdown and provenance
for the parent to use in repository artifacts. See [reading existing chats](USAGE.md#read-existing-chatgpt-chats).

## Global research queue and pacing

All managed research shares one active slot, enforced by a SQLite unique constraint and the single
daemon worker. New threads may be queued, but only the head request begins pacing after the active
request finishes. Every outgoing prompt waits `ceil(words / words_per_minute * 60)` plus
`fixed_pause_seconds`, plus a random 0–`pause_jitter_seconds` (default 30) extra seconds per request.
Set jitter to 0 for a fixed pause. The random offset stays stable for that request across restarts.
This is a pre-send wait: the complete message is inserted at once, not typed character by character.
Neither another thread/project nor time already spent queued grants credit.
Restarting conservatively restarts the current pacing interval and refreshes its `send_after` estimate.
Timeouts and ambiguous submissions don't authorize overlapping generations. Read-only source imports
send no prompts and don't consume this budget.

### Shared browser action pacing

All automatic browser work uses the same durable scheduler, across projects, research requests,
imports, cleanup, and recovery. Each menu click, selection, keyboard action, text insertion, or
scroll waits for the UI cooldown (randomized 2–5 seconds by default). Send, Start research, and
deletion confirmation also respect the account-action cooldown (1–3 minutes). Full navigation,
including reopening a lost tab or a cleanup fallback, respects the page-load cooldown (5–10 minutes).
Each configured range must be ordered and within 1–3600 seconds.

These are overlapping deadlines, not delays added together. A menu click doesn't extend the
page-load deadline, and the page-load cooldown doesn't prevent ordinary controls from being used.
Page loading can overlap prompt composition. Deadlines use millisecond precision, persist in
`browser-pacing.json`, and aren't redrawn or extended by polling. A page is never reloaded merely
because its cooldown expired. Passive DOM inspection and local tab closure consume no action slot.
Stop is an urgent safety exception; cancellation also interrupts a pending action before it executes.
Imports and cleanup wait while a managed generation is being observed so they cannot delay Stop.
The worker retains its CDP connection and renderer focus emulation across polling and scheduling
yields. It does not repeatedly toggle page visibility while observing a request; those transitions
can cause ChatGPT to refetch background data even without a page load. The connection is released
when the request finishes, needs attention, or is replaced, and recreated only when needed.

Status and result retrieval are browser-free. Results provide `next_wait_seconds` and, for managed
requests, `next_browser_action_at` (Unix seconds) as polling hints, not commands to refresh ChatGPT.
The service rereads browser pacing settings live; queued requests keep their existing composition policy.

### Navigation diagnostics

An independent, passive CDP observer records document requests, failed ChatGPT requests, and HTTP
errors while connected. Before navigating a newly created background tab, the service waits for
the observer's network-enable acknowledgement so it can capture the initial document request.
Observer readiness never activates a window or sends an account request. An observed HTTP 429 pauses all ordinary action classes for `Retry-After`
(seconds or an HTTP date), or 15 minutes when that header is unavailable. It does not resend work.
Health exposes the shared deadlines, counters, and observer connection state under
`service_status.browser`; raw deadline fields ending in `_ms` use Unix milliseconds.

`browser-events.jsonl` records timestamped admission, UI attempts, navigation reasons, RPC operations,
and sanitized network events. It retains one previous file and rotates at roughly 1 MiB per file.
It never logs URLs, request/response bodies, prompts, cookies, or authorization headers. Keep these
files local and out of source control. Observer downtime is explicit; neither missing historical
logs nor an observation with zero errors proves that throttling never occurred.

## Idle browser lifecycle

`chrome_auto_close` defaults to `true`. `chrome_idle_timeout_minutes` defaults to 30 and accepts
1–525600 minutes. Only the owned research-profile Chrome is closed. Service browser interactions
reset the timer; local database reads and timer checks do not. Response observation counts as
activity, and active or unresolved submissions prevent automatic closure.

The running server checks these settings and all cleanup timers about once a minute even when no
agent calls tools. Due remote deletions relaunch Chrome as needed. Browser-backed checks and new
messages also relaunch it and reopen the persisted conversation URL when its tab is gone. Profile
login survives closure. Local `status`, listing, and saved-result retrieval remain browser-free.
Idle configuration is read live rather than from an older request's config snapshot.

## Two different model settings

The OpenCode researcher inherits its parent session's model by default. You can set an agent-specific
override in OpenCode if you prefer. It writes prompts, waits, and summarizes results.

The config above controls ChatGPT's website. `default` keeps the account's model choice. Other
values are UI labels, not API model IDs. The service prefers Extra High and falls back to High.
Deep Research uses its own model/reasoning controls where ordinary thinking levels aren't available.
Missing required controls produce an error rather than silently selecting a lower setting.

New requests read the current settings; queued requests keep their original snapshot.

## Put new chats in a ChatGPT project

Open the project in ChatGPT and copy its URL into `chatgpt_project_url`:

```json
{"chatgpt_project_url": "https://chatgpt.com/g/g-p-<project-id>/project"}
```

This applies across opted-in Git repos on this server. Each new thread remembers its destination,
so changing the setting doesn't move existing chats. `null` creates ordinary chats. The service
checks the project page before sending and fails if it can't verify it. Project instructions,
files, and memory may influence the answer.

## Transcripts and cleanup

The data directory holds `state.sqlite`, the separate Chrome profile, service/browser records,
and saved transcripts at `archives/<thread-id>/thread.md`. Structured data stays in SQLite.

Prompts are saved before sending at `transcripts/<thread-id>/<request-id>/prompt.md`. Each completed
exchange is saved alongside it as `exchange.md`, while the chat is still active.
Captured partial responses also persist in SQLite. Missing exports are retried from that database
after a restart or temporary disk failure; they don't require the ChatGPT conversation to exist.

Deleting a conversation manually in ChatGPT doesn't delete local records. An answer deleted before
it was captured cannot be recovered; resume/follow-up operations may fail for a deleted chat.

Each managed chat expires after a random 1–7 days of inactivity, or after the tenth response.
`chat_deletion_delay_min_hours` sets the lower bound (default 24 hours), and
`chat_deletion_delay_max_hours` sets the upper bound (default 168 hours). Both are inclusive,
must be between 1 and 8760 hours, and the maximum must be at least the minimum. Equal bounds
give a fixed delay. Changing either setting recalculates eligibility for active chats.
The offset is derived from the chat's persisted random ID, so polling and restarts don't redraw it.
New activity moves the deadline forward using the same offset. Resume, follow-up admission, and
cleanup use the same deadline, including for existing active chats. Expiry makes a chat eligible
for cleanup; spacing and failed-deletion retries may delay actual removal.
The service verifies a final full transcript before deleting the conversation.
This isn't ChatGPT's **Archive chat** feature.

The old keys `remote_chat_inactivity_hours`, `remote_chat_inactivity_min_hours`, and `inactivity_hours`
remain aliases for the minimum; `remote_chat_inactivity_max_hours` remains an alias for the maximum.
`transcript_retention_days` remains an alias for local retention.
Local copies expire after `local_transcript_retention_days` (default 30) since last thread activity, once remote
deletion is confirmed. Both files and database history are removed, so exports cannot recreate them.
Pending remote deletion is retained. Listing and polling don't extend inactivity; active or
ambiguous work isn't deleted by expiry.

If saving fails, deletion is blocked. Cleanup retires at most one chat per pass, waiting at least
60 seconds after completion before the next pass, including after startup or wake. Cleanup failures
also retry after 1, 2, 4, 8, 16, 32, then 60 minutes, with an hourly cap. Per-chat retry schedules
survive restarts; successful cleanup resets the counter. Retries verify saved transcripts and
consume no prompts.
Three consecutive `deletion_access_unavailable` results stop automatic retries in
`deletion_inaccessible`, including already-exhausted records on upgrade. This is an unconfirmed
permanent cleanup failure, not evidence that the chat was deleted. Local history remains retained;
health reports manual review separately from retryable cleanup. Only an explicit `retire` operation
reopens a bounded retry series. Other errors retain the ordinary backoff.

Deletion confirmation is checkpointed before the click. A missing acknowledgement permits one
paced, observation-only reopening of the exact chat, never another Delete click. If recovery still
has no identity-bound evidence, `deletion_unconfirmed` stops automatic browser work. An explicit
scoped `retire` can request another observation-only recovery; it doesn't clear the checkpoint.

### Accessing transcript files

Active chats have a combined capture at `transcripts/<thread-id>/thread.md`; final copies live at
`archives/<thread-id>/thread.md`. Imports use `imports/<read-request-id>/<chat-index>.md`.
Keep the database as well as transcript folders when backing up: agent history retrieval uses
SQLite, while Markdown files are independently readable. No JSON transcript exports are produced.

Tool responses expose `local_transcript.markdown.path`. The plugin publishes disposable Markdown
copies under OpenCode's reported temporary directory so parent agents can read them using normal
managed-temp access. Custom read-deny rules still apply. The researcher hands off only the plain
absolute path for each transcript, without a metadata section or instructions for the parent's output.
These paths are not clickable URLs.

Temporary copies regenerate on retrieval while the database record is retained. The plugin removes
its own unused copies older than 30 days when publishing. Ask your coding agent to copy a transcript
into the repo if you want a permanent artifact. A capture may be incomplete; see
[capture limitations](USAGE.md#read-existing-chatgpt-chats).

### Response deadlines and Chrome discovery

Normal responses have a 15-minute deadline; Deep Research has 30 minutes. A timeout preserves
captured results and keeps observing rather than resending. Leave `chrome_path` as `null` for
automatic discovery, or supply the Chrome executable path.
