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
  "inactivity_hours": 24,
  "search_timeout_seconds": 900,
  "deep_research_timeout_seconds": 3600,
  "chrome_path": null
}
```

## Two different model settings

The OpenCode researcher uses `openai/gpt-6-astra#low`, set in its agent Markdown frontmatter.
It writes prompts, waits, and summarizes results.

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

After 24 hours of inactivity or the tenth response, the service saves a local transcript before
deleting the managed ChatGPT conversation. This isn't ChatGPT's **Archive chat** feature. Local
copies remain until you remove them. Listing and polling don't extend inactivity; active or
ambiguous work isn't deleted by expiry.

If saving fails, deletion is blocked. Cleanup failures retry after 1, 2, 4, 8, 16, 32, then 60 minutes,
with an hourly cap. Retry schedules survive restarts. A failed remote cleanup stops that batch;
successful cleanup resets the counter. Retries verify saved transcripts and consume no prompts.
