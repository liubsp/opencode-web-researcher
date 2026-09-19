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
and saved transcripts at `archives/<thread-id>/thread.md` and `thread.json`.

Prompts are saved before sending at `transcripts/<thread-id>/<request-id>/prompt.json`. Each completed
exchange is saved alongside it as `exchange.md` and `exchange.json`, while the chat is still active.
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

If saving fails, deletion is blocked. Cleanup failures retry after 1, 2, 4, 8, 16, 32, then 60 minutes,
with an hourly cap. Retry schedules survive restarts. A failed remote cleanup stops that batch;
successful cleanup resets the counter. Retries verify saved transcripts and consume no prompts.
