# Conditional connections

A connection between two steps can carry a condition. When the condition fails,
the connection does not fire and the branch behind it does not run. A connection
with no condition always fires, which is how every workflow behaved before this
existed — no existing workflow changes.

```text
                        ┌─(output contains "FAIL")──> [Triage] ──> [Slack] ──> [Output]
[Agent: run tests] ─────┤
                        ├─(output contains "PASS")──> [Create PR]
                        └─(no condition)───────────> [Write file]
```

## Authoring

Click a connection on the canvas. Keyboard users tab to it and press Enter. The
editor has three controls:

| Control | Meaning |
| --- | --- |
| Compare | Which value to test |
| Test | How to test it |
| Value | What to test it against (hidden for "is empty" / "is not empty") |

`Match upper and lower case exactly` is off by default, so `contains FAIL` also
matches `fail`.

A conditional connection renders with a dashed stroke and a chip showing the
condition. **Remove condition** returns it to an ordinary connection.

## What can be compared

| Compare | Value |
| --- | --- |
| Output of *step* | What the step this connection leaves produced on this run |
| Run context | Everything gathered so far on this run, across earlier steps |
| Trigger payload | The payload that started the run; empty for a manual run |

Step status and exit code are deliberately absent. A step that errors ends the
whole run before any downstream connection is evaluated, so a condition on them
could never be true. They become available if failure-tolerant steps are added.

## Tests

| Test | True when |
| --- | --- |
| contains / does not contain | The value appears anywhere in the compared text |
| equals / does not equal | The whole compared text matches, ignoring surrounding whitespace |
| matches regex | A regular expression matches. An invalid pattern is reported in the editor and evaluates false at run time — it never fails the run |
| is empty / is not empty | The compared text is blank (whitespace only counts as blank) |

Values are capped at 512 characters.

## How a run behaves

Three rules decide everything:

1. A connection fires when its source step produced a result **and** its
   condition is absent or true.
2. A step runs when it has no incoming connections, or when at least one
   incoming connection fired. Several firing connections still run it once.
3. A step is skipped when it has incoming connections and none fired.

Rule 3 is what makes a whole branch disappear: a skipped step produces no
result, so its own connections cannot fire, so the next step skips too.

Consequences:

- **Fan-out** runs every branch whose condition passes, not just the first.
- **A join** downstream of one live and one skipped branch still runs, because
  one firing connection is enough.
- **A skipped step** adds nothing to the run context, saves no output, and
  writes no memory. It appears in the activity panel and run history as
  `skipped`, with the conditions that did not fire.
- **The run still completes.** Skipping is not failing, even when every branch
  is skipped.

## Sharing

A shared template carries its conditions. Because a build that ignored them
would run every branch and look correct while doing the wrong thing, a template
containing a condition declares schema version 2 and older builds refuse it. A
workflow without conditions still exports as version 1 and stays readable
everywhere. An unrecognized source or test is rejected on import rather than
ignored.

See [workflow sharing](workflow-sharing.md) for the rest of the template
contract.
