# Scheduling on the canvas

A workflow's schedule is a node on its canvas, to the left of the steps it
starts. The node is a view of the one schedule record the app keeps: it reads
cron, enabled state, and the next run from that record and writes back to it.
It is not a step and never enters the saved graph, so a workflow that is run by
hand behaves exactly as it did before.

```text
[Schedule] ──→ [Input] ──→ [Agent] ──→ [Output]
 Weekdays at 07:00 UTC
 Next: Thursday, 10 Sept 2026 at 09:00 CEST
 [On]
```

## Managing a schedule from the canvas

| Action | How |
| --- | --- |
| Add | **Add schedule** in the workflow toolbar |
| Edit | Click the node, its pencil, or **Edit schedule** in the toolbar |
| Turn on/off | The switch on the node |
| Remove | The node's trash button, `Backspace` on a selected node, or the form's **Remove** |

The form is the same one the sidebar and the Schedules overview open: repeat,
day, time, an optional cron expression, the policy for a time that passed while
Alfred was closed, and **Remove**. Saving the form again edits the same
schedule — a workflow has one schedule, so a second **Add schedule** turns into
**Edit schedule**.

Dragging the node stores only where it is drawn. It never changes when the
workflow runs or what runs.

## Why the node connects to several steps

A schedule starts the whole workflow, not one branch of it. The arrows are
generated from the saved graph: one to every step that has no incoming
connection. Change the graph and the arrows follow. They cannot be selected,
moved, or deleted, because there is nothing of their own to edit — a single
arrow to a step that has no incoming connection is the honest picture:

| Graph | Arrows |
| --- | --- |
| One chain | One, to its first step |
| Two disconnected branches | One per branch — both run |
| No steps yet | None, with a reminder to add a step |

A workflow with an empty or invalid graph is left exactly as it is. Open it and
the schedule node appears; nothing is added to the graph on your behalf.

## Times are stored in UTC

A recurrence is stored as a six-field cron expression in UTC, which is what the
scheduler evaluates. The canvas and the Schedules list show the next occurrence
in your own timezone and name the zone, and the recurrence line states its UTC
basis (`Weekdays at 07:00 UTC`) instead of pairing a local clock time with a
UTC weekday.

The order of an edit is:

1. You pick a wall-clock time in the form.
2. The form converts it to the matching UTC time for the reference date and
   stores that cron.
3. Every display of it converts back for your zone.

A recurrence therefore cannot promise the same local hour across a daylight
saving change: `09:00` in your zone today is stored once and read back with
whatever offset applies on the day it fires. Named-timezone recurrence is a
separate change, not a hidden rewrite inside this one.

For a rule that is not everyday shorthand — unusual days, months, or seconds —
the form asks the backend, using the same parser the scheduler runs on, for the
next occurrence. The editor's own lightweight preview is never treated as
authoritative for those expressions.

## What the node shows

| State | Node |
| --- | --- |
| On, armed | Next run with date, time, and zone |
| On, not yet armed | "Next run is being scheduled" — the ticker arms it within 20 seconds |
| Off | "Off — nothing runs until it is on"; no next run is invented |
| Invalid expression | The saved expression and "Not a valid schedule" |
| Empty workflow | "Add a step for this schedule to start" |

A failed write is reported on the node, and the persisted state wins: the node
re-reads the schedule so it can never claim a value the backend refused. Turning
a schedule on and off is a separate, enabled-only write — cron and the
missed-run policy are left untouched.

## The Schedules overview

**Schedules** in the sidebar lists every workflow's schedule, across workflows
and folders. **Open** switches to the workflow and focuses its schedule node;
**Edit** additionally opens the schedule form. **Remove** deletes just the
schedule — the workflow, its steps, and its manual runs stay as they are.

Nothing on that page is a second copy of the schedule: it reads the same
records the canvas node does.

## Licensing

Schedule authoring is a paid capability on official builds, and manual runs
stay free. The free slot covers one schedule; a schedule outside it is visible
and removable but its switch cannot turn it on. Source builds (built with
`ALFRED_DISABLE_LICENSE_GATING=1`) are never gated.

## Shared workflows

A shared template carries its schedule recipe, and the recipient's copy arrives
**disabled** with a schedule node to inspect — a shared workflow never starts
running on someone else's machine. The node and its arrows are presentation, so
they are never part of an exported graph.
