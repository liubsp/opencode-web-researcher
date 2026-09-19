# Settings and saved data

Run the server's `configure` command to create the shared `config.json` and print its path.
`doctor` shows the current paths and settings without starting research.

By default, the config file is at:

- Windows: `%LOCALAPPDATA%\web-research-opencode\config.json`
- macOS: `~/Library/Application Support/web-research-opencode/config.json`

For tests or a separate server instance, set `WEB_RESEARCH_HOME`. The plugin and executable must
inherit the same value. This selects a different data directory, including its profile and queue.

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
  "chrome_path": null,
  "chrome_auto_close": true,
  "chrome_idle_timeout_minutes": 30
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
`fixed_pause_seconds`; neither another thread/project nor time already spent queued grants credit.
Restarting conservatively restarts the current pacing interval and refreshes its `send_after` estimate.
Timeouts and ambiguous submissions don't authorize overlapping generations. Read-only source imports
send no prompts and don't consume this budget.

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

After 24 hours of inactivity or the tenth response, the service verifies a final full transcript before
deleting the managed ChatGPT conversation. This isn't ChatGPT's **Archive chat** feature.

The old config keys `inactivity_hours` and `transcript_retention_days` remain accepted as aliases.
Local copies expire after `local_transcript_retention_days` (default 30) since last thread activity, once remote
deletion is confirmed. Both files and database history are removed, so exports cannot recreate them.
Pending remote deletion is retained. Listing and polling don't extend inactivity; active or
ambiguous work isn't deleted by expiry.

If saving fails, deletion is blocked. Cleanup retires at most one chat per pass, waiting at least
60 seconds after completion before the next pass, including after startup or wake. Cleanup failures
also retry after 1, 2, 4, 8, 16, 32, then 60 minutes, with an hourly cap. Per-chat retry schedules
survive restarts; successful cleanup resets the counter. Retries verify saved transcripts and
consume no prompts.

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
