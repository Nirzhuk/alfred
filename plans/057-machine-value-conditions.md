# Plan 057: Machine values as condition subjects

## Status and scope

- Status: IMPLEMENTED — frontend build, helper tests, condition/runner tests,
  and template-contract tests green. A probe smoke run exercised every native
  path on macOS (IOKit power, AppKit frontmost app, clipboard, network,
  hostname/username, file, environment). Desktop visual verification of the
  edited modal and a live run are still outstanding.
- Priority: P1. Effort: M.
- Requested on: 2026-11-20.
- Baseline: Plan 053 conditional connections implemented in the working tree.
- Depends on: Plan 053's edge condition model, runner level gate, and template
  condition contract.

Plan 053 could only compare data produced by the run: step output, run context,
and trigger payload. Weekday checks required a regular expression, and nothing
could depend on the machine Alfred runs on. This plan adds machine values as a
fourth subject category and gives each field the operators its value type
supports.

It adds no expression language, no loops, no failure-tolerant steps, and no
prompt variables. Machine values decide whether a connection fires and nothing
else.

## Data model

`EdgeCondition` gains an optional `field`, `key`, and `value2`:

```ts
type EdgeCondition = {
  source: "output" | "context" | "trigger" | "system";
  field?: SystemField;   // required for source: "system"
  key?: string;          // env variable name or file path
  operator: EdgeConditionOperator;
  value: string;
  value2?: string;       // upper bound for between / notBetween
  caseSensitive?: boolean;
};
```

`SystemField` is one of `weekday`, `date`, `time`, `timezone`, `os`,
`hostname`, `username`, `powerSource`, `battery`, `network`, `path`, `env`,
`frontmostApp`, `clipboard`.

Each field declares its accepted operators. Run sources keep exactly the seven
Plan 053 text operators; typed operators are rejected on them so a step output
never silently gains date semantics.

## Evaluation

`src-tauri/src/system_facts.rs` resolves facts; `runner/conditions.rs` evaluates
them without I/O.

- **Clock is pinned at run start.** `ClockFacts` holds the local date,
  minute-precise time, weekday, and IANA time zone. A run crossing midnight
  cannot change a day-of-week branch mid-flight.
- **Everything else is read live at each gate.** Machine identity, file
  metadata, power, battery, network, frontmost app, clipboard, and environment
  variables are resolved through `SystemRequirements`, which is collected once
  from the graph. A field no condition names is never probed.
- **Sequential runs resolve before each step; parallel levels resolve once
  before the shared gate.** Every branch in a level gates before any of them
  runs, so live values are consistent across the fan-out.
- **Unavailable values are false.** Unreadable facts never satisfy `isEmpty`.
  The skip reason names the field and the reason.
- **Network means a default route exists.** A UDP connect performs no traffic
  and needs no DNS. It is not a reachability or bandwidth test.
- **Frontmost app is macOS-only.** Other platforms report unavailable rather
  than a fabricated name.
- **Paths resolve from the workflow working directory**, falling back to the
  process directory. Modified times compare in the local zone.

The typed helpers use `chrono` and `chrono-tz` (already dependencies), `battery`
for power, `whoami` for identity, `arboard` for clipboard, and AppKit's
`NSWorkspace` for the frontmost app.

## Contract

A machine-value condition requires schema version 3. Version 2 still carries
run-value conditions and version 1 carries none, so older links keep importing.
Unknown fields, mismatched field/operator pairs, a missing path or variable key,
an invalid date/time, and out-of-range battery levels are rejected on import.
Fixtures:

- `contracts/workflow-template/fixtures/system-condition-edge.json`
- `contracts/workflow-template/fixtures/invalid-system-condition-field.json`

## UI

The connection editor's subject select gains grouped options: Clock, Machine,
Power & network, Files, Environment, Active app & clipboard. The value control
switches by field: weekday checkboxes, date and time inputs, a two-bound range,
a battery number, a native select for OS/power/network, a path/variable key
input, and a date-time picker for modification tests. The editor shows the
currently resolved value through `preview_condition_subject`, which reuses the
runner probes and the workflow's working directory.

No machine value is added to prompts, outputs, memory, or run history.

## Acceptance

| Scenario | Expected result |
| --- | --- |
| Weekday condition, weekend run | Weekend branch runs; weekday branch skipped |
| Weekday condition, Friday run | Friday matches `is one of Fri`; Sat/Sun does not |
| Time window 09:00–17:00 | True at 09:00 and 17:00; false outside |
| Overnight window 22:00–06:00 | True at 23:00 and 05:00; false midday |
| Run crosses midnight | Day-of-week branch uses the start day, not the new day |
| Battery at least 20 | True at 20 and above; false below; unavailable on a desktop |
| Network equals online | Matches the default-route probe |
| Missing file | `exists` false, `does not exist` true |
| File created by an earlier step | A later gate sees it, because metadata is read live |
| Unset environment variable | `is empty` true; `equals ""` true |
| Clipboard unreadable | Condition false, skip reason names the reason |
| Frontmost app on non-macOS | Unavailable, never a fabricated name |
| Workflow without machine conditions | No battery, network, or clipboard probe |
| Export/import with a machine condition | Field, key, and bounds survive; template is v3 |
| Older build opens a v3 template | Refused explicitly |
| Unknown field or mismatched operator | Rejected on import |
| Editor preview | Shows the same value and path base the runner will use |

Run `bun run check` and `git diff --check` before handoff. Runner behavior must
be demonstrated by an actual desktop run; static source assertions do not count.
