# Using the researcher

Ask your usual OpenCode agent to use `web-researcher`. Give it the question and any constraints
that matter: platform, versions, date range, or preference for official sources.

> use web-researcher to compare these libraries, then check the weakest claim with one follow-up

For Deep Research, ask explicitly. A complicated question alone doesn't enable it.
To continue earlier work, give the agent the thread ID from its report and ask it to resume.

To reuse findings without sending another prompt, ask it to list previous research (including
retired threads) and read the selected thread with `research_archive`. This reads the local database
and works after remote deletion. For exact prompts and file locations, see
[Reuse earlier research](../README.md#reuse-earlier-research). Resuming a remote conversation is
separate and requires that the chat still exists and the thread remains eligible.

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

## When something goes wrong

Use the installed server's full path, or put its directory on PATH. These commands also work
with the source-build executable named `web-research`:

```sh
web-research-server doctor
web-research-server status
web-research-server browser-check
web-research-server browser-inspect
web-research-server login
web-research-server shutdown
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

Only the researcher gets these tools. See [validation status](VALIDATION.md) for tested coverage
and the [live-test report](AGENT-LIVE-TEST.md) for actual prompt reviews and recovery cases.
