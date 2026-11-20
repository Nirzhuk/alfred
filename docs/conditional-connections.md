# Conditional connections

A connection between two steps can carry a condition. When the condition fails,
the connection does not fire and the branch behind it does not run. A connection
with no condition always fires, which is how every workflow behaved before this
existed — no existing workflow changes.

```text
                        ┌─(output contains "FAIL")──> [Triage] ──> [Slack] ──> [Output]
[Agent: run tests] ─────┤
                        ├─(weekday is one of Sat, Sun)──> [Weekend report]
                        └─(battery is at least 20%)──────> [Start render]
```

## Authoring

Click a connection on the canvas. Keyboard users tab to it and press Enter. The
editor has three controls:

| Control | Meaning |
| --- | --- |
| Compare | Which value to test |
| Test | How to test it |
| Value | What to test it against. The input changes with the subject: day checkboxes, a date picker, a time window, a battery threshold, or a text field |

`Match upper and lower case exactly` is off by default, so `contains FAIL` also
matches `fail`. It is offered only for text comparisons.

While a machine value is selected, the editor shows the value it reads right
now, for example `Current: Friday` or `Current: Not found`. That preview uses
the same probes as the runner; it is not a guess. Values that cannot be read on
this machine say so instead of showing a fake value.

A conditional connection renders with a dashed stroke and a chip showing the
condition. **Remove condition** returns it to an ordinary connection.

## Run values

These were the original sources. They read data produced by the run itself.

| Compare | Value |
| --- | --- |
| Output of *step* | What the step this connection leaves produced on this run |
| Run context | Everything gathered so far on this run, across earlier steps |
| Trigger payload | The payload that started the run; empty for a manual run |

Step status and exit code are deliberately absent. A step that errors ends the
whole run before any downstream connection is evaluated, so a condition on them
could never be true.

## Computer values

These read the machine running Alfred. Values marked **run start** are captured
once, before the first step, so a long run cannot cross midnight and change
which branch fires. Values marked **live** are read when the connection is
evaluated.

| Compare | Read at | Value |
| --- | --- | --- |
| Day of week | run start | Local weekday name (`Monday` … `Sunday`) |
| Date | run start | Local date (`YYYY-MM-DD`) |
| Time of day | run start | Local 24-hour time (`HH:MM`) |
| Time zone | run start | IANA name, for example `America/New_York` |
| Operating system | live | `macos`, `windows`, or `linux` |
| Hostname | live | This computer's hostname |
| Username | live | The account running Alfred |
| Power source | live | `AC` or `Battery` |
| Battery level | live | Charge percentage, 0–100 |
| Network | live | `online` when a default network route exists, otherwise `offline` |
| File or folder | live | Existence, file/folder kind, and modification time |
| Environment variable | live | A named variable of the Alfred process |
| Frontmost app | live | The app in front; macOS only |
| Clipboard text | live | The clipboard's current text |

The local time zone is the computer's time zone. A run does not carry a
per-workflow zone override, and a scheduled run that fires near midnight uses
the day the run actually started.

### Unavailable values

A value the computer cannot provide — the frontmost app on Windows or Linux, a
clipboard held by another process, a machine without a battery — makes the
condition false. It never makes a branch run by accident. The skipped step in
run history names the reason, for example `The clipboard is unavailable`.

## Tests

| Test | True when | Subjects |
| --- | --- | --- |
| contains / does not contain | The value appears anywhere in the compared text | text values |
| equals / does not equal | The whole compared text matches, ignoring surrounding whitespace | text, date, OS, power source, network, time zone |
| is one of / is not one of | The value matches any comma-separated candidate | text, weekday, time zone |
| matches regex | A regular expression matches. An invalid pattern is reported in the editor and evaluates false at run time — it never fails the run | text values |
| is empty / is not empty | The compared text is blank (whitespace only counts as blank) | text values |
| is before / is after | The value is before or after a date or time | date, time |
| is between / is not between | The value is inside or outside a range, inclusive | time, battery |
| is at least / is at most | The number is at or above / at or below the threshold | battery |
| exists / does not exist | The path exists or does not exist | file or folder |
| is a file / is a directory | The path exists and is that kind | file or folder |
| was modified before / after | The file's modification time is before or after a local date and time | file or folder |

A time window whose start is later than its end wraps across midnight, so
`between 22:00 and 06:00` is an overnight window. Time comparisons use the
run-start clock, not the wall clock at the moment a branch is reached.

Text comparisons ignore surrounding whitespace for `equals`. `is one of` splits
on commas and trims each candidate; case sensitivity applies to each candidate.
Values are capped at 512 characters.

## File paths and variables

A relative path resolves from the workflow's working directory. When the
workflow has no working directory, it resolves from the process directory, the
same rule the runner uses everywhere else. Absolute paths are used as written.
Modification times are compared in the local time zone.

An unset environment variable is an empty value: `is empty` is true and
`equals ""` is true. A value the process cannot read is unavailable instead, so
it is false for every test.

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
- **Machine values are read only when a condition names them.** A workflow
  without a clipboard or battery condition never touches either.

## Privacy

Machine values are read locally, used only to decide whether a connection fires,
and discarded with the run. They are never added to a model prompt, a step
output, memory, or run history. The editor's current-value preview is the only
place a resolved value is shown.

## Sharing

A shared template carries its conditions. A template with a run-value condition
declares schema version 2; one with a machine-value condition declares version
3. Older builds refuse both rather than importing a workflow that looks correct
while running the wrong branch. A workflow without conditions still exports as
version 1 and stays readable everywhere. An unrecognized source, field, or test
is rejected on import rather than ignored.

See [workflow sharing](workflow-sharing.md) for the rest of the template
contract.
