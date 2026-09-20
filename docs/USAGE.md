# Using the researcher

Ask your usual OpenCode agent to use `web-researcher`. Give it the question and any constraints
that matter: platform, versions, date range, or preference for official sources.

> use web-researcher to compare these libraries, then check the weakest claim with one follow-up

For Deep Research, ask explicitly. A complicated question alone doesn't enable it.
To continue earlier work, describe the investigation and ask the researcher to find and resume it.
You can also supply a thread ID if you have one.

The agent starts with a short, conversational question and follows the evidence with useful
corrections, narrower questions, or new directions. It doesn't dump the full task specification
into its opening message or enforce a two-follow-up ceiling. Useful raw logs can be pasted unchanged
in separate blocks with short questions; this guidance limits dense agent-written prose, not raw
evidence. Each chat has a
hard ten-prompt limit. Waiting and reading results are free. Messages to ChatGPT are casual, with
no final period; the report back to you uses clear prose and source links.

Be patient: queue time and the composition delay come before ChatGPT's own research. The agent
waits on the existing request instead of sending reminders. Cited links aren't automatically
independently verified; the report should say which checks were actually performed.

## Read existing ChatGPT chats

Supply conversation URLs or IDs accessible to the signed-in research Chrome account:

> use web-researcher to read these chats: `<chat-url-1>`, `<chat-url-2>`, `<chat-id-3>`. Summarize their agreements and contradictions without sending a new prompt

> use web-researcher to read these chats, research any gaps in a separate chat, then combine the findings into `docs/research.md`

Source chats are read without sending messages or deleting them. Further research uses a separate
managed chat; your coding agent handles repository files and Git. Read-only captures don't consume
the prompt budget or incur a composition delay.

Captures cover the rendered current branch, including extracted text and source links. Unloaded
history, alternate branches, attachments, and separate report panels may be absent. The researcher
reports inaccessible chats and capture limitations; this isn't a guaranteed full account export.

## Reuse earlier research

> use web-researcher to list this repo's previous research, including retired chats, and retrieve the findings about Chrome profiles without sending anything to ChatGPT

Saved results remain readable after remote deletion, within local retention. Listing and retrieval
don't need Chrome or consume prompts. Continuing the remote conversation requires that the chat
still exists, is unexpired, and has remaining prompt budget.

The researcher can provide a local transcript path. Ask your coding agent to copy it into the repo
for a permanent artifact. See [transcripts and cleanup](CONFIGURATION.md#transcripts-and-cleanup)
for file locations, backups, and retention.

## When something goes wrong

Use the installed server's full path, or put its directory on PATH. Source builds use the same name:

```sh
opencode-web-researcher doctor
opencode-web-researcher status
opencode-web-researcher browser-check
opencode-web-researcher browser-inspect
opencode-web-researcher login
opencode-web-researcher shutdown
```

- **Agent missing:** check the installed Markdown file and reload the correct OpenCode location.
- **Executable missing:** rerun setup with the right binary path.
- **Login needed:** use `login` and sign in to the dedicated research profile.
- **Mode/model/reasoning unavailable:** check your account's controls and shared settings. A ChatGPT
  UI change may need an adapter update.
- **Submission unknown or needs attention:** fix the browser issue, then ask the agent to reconcile
  the existing request. That only observes; it doesn't resend. Ambiguous work pauses new dispatch.
- **Timed out:** partial results remain available and observation continues. Don't restart the chat.

`browser-check` exercises a blank tab and checks minimized state and `navigator.webdriver`.
`browser-inspect` reports readiness and control labels, not transcripts. Don't navigate or manually
send messages in an active research tab. Stop the daemon before replacing its executable on Windows;
Chrome remains open after shutdown. Cancellation cannot undo a prompt already sent.

## Agent tools

| Tool | Purpose |
| --- | --- |
| `research_start` | Start a thread; keep the same request key for retries |
| `research_send` | Send a new follow-up in that thread |
| `research_wait` / `research_get` | Wait up to 60 seconds / inspect results |
| `research_list` / `research_resume` | Find threads / access an unexpired thread without sending |
| `research_archive` | Read saved local transcripts |
| `research_cancel` | Cancel queued work or request generation stop |
| `research_reconcile` | Observe an ambiguous submission after the underlying issue is fixed |
| `research_read_chats` | Capture 1–10 supplied chat references without sending messages |
| `research_read_content` | Read a saved import's Markdown in pages |

Imports return a read request ID. Use `research_wait`/`research_get` for per-chat state and errors,
then `research_read_content` with `chat_index`, following `next_offset`. Retry starts with the same
request key; a new key creates a fresh capture. Reading saved content doesn't reopen Chrome.
`research_archive` reads local history for active or retired threads; it doesn't invoke ChatGPT's
Archive chat feature.

Only the researcher gets these tools. See [validation status](VALIDATION.md) for tested coverage
and the [live-test report](AGENT-LIVE-TEST.md) for actual prompt reviews and recovery cases.
