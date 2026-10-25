# Agent guidance

- Follow work through to the requested outcome; do not stop at an intermediate fix or verification layer.
- Before declaring success, test the real user-facing path. Distinguish unit, integration, installation, and live behavior evidence.
- Investigate the cause of failures and prevent recurrence proportionately; avoid symptom-only workarounds.
- Run the repository's complete local validation before committing or pushing, including static checks and tests.
- After pushing, confirm required remote checks pass. If they fail, inspect and resolve them before calling the work done.
- Account for cached or long-lived processes when validating installed changes; confirm the running system loaded the new code.
- Keep status reports precise: state what is verified, what is not, and any remaining blocker.
- Keep changes and guidance concise, generic, and limited to the task.
