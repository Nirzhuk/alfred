# Plans 047–048 UI QA results

Date: 2026-09-09

## Environment and evidence

- Source snapshot: `85f0d64` (`git rev-parse --short HEAD`); root-owned uncommitted frontend changes were present in the isolated workspace at test time.
- Fixture: `http://127.0.0.1:1422/tests/ui/memory-context-plans.html` served by the root-owned Vite session 17731.
- Browser: Argent `chromium-cdp-9222`, Chrome 152.0.7977.84, isolated CDP profile/PID 56771.
- All calls below were synthetic IPC fixture calls. This evidence covers browser wiring and visible UI behavior; it does not prove native Tauri transport, Rust handlers, real persistence, provider access, or live model behavior.
- Argent discovery was run before every tap. Coordinates in this report are normalized `[0,1]` frames from `describe`, not screenshot-derived pixels.

## Picker / node settings

### PASS — light, normal, post-restart linked selection

After the isolated Vite listener was restarted, keyboard traversal reached the linkable row and `Space` selected it. The fixture recorded:

```json
{
  "selection":["context-fact","context-report","context-expired","context-retracted","context-deleted","context-linkable"],
  "calls":[
    {"command":"list_linkable_memories","payload":{"workflowId":"context-qa-workflow"}},
    {"command":"link_memory","payload":{"workflowId":"context-qa-workflow","memoryId":"context-linkable"}}
  ],
  "errors":[]
}
```

Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788962433861-21551.png`.

### PASS — light, empty

The dialog rendered `Selected 0`, `Select a memory below to give this step context`, and `Available 5`; the fixture recorded only the initial `list_linkable_memories` request and selection remained `[]`.

Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788962804373-21551.png`.

### PASS — dark, lookup failure and retry

The dialog rendered `Could not load other workflows.` with a visible `Retry` action, while retaining the existing selected rows and their expired/retracted/unavailable explanations. The retry was discovered at normalized frame `(0.661, 0.883, 0.042, 0.038)` and tapped at its center. The fixture recorded two `list_linkable_memories` calls and no uncaught error:

```json
{"calls":[
  {"command":"list_linkable_memories","payload":{"workflowId":"context-qa-workflow"}},
  {"command":"list_linkable_memories","payload":{"workflowId":"context-qa-workflow"}}
],"errors":[]}
```

The picker dialog frame was `(0.285, 0.070, 0.431, 0.854)`; the failure alert frame was `(0.297, 0.883, 0.407, 0.038)`. Screenshots: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788962810973-21551.png` and retry state `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788962816609-21551.png`.

## Library

### PASS — light report, original-file failure is safe and visible

The report details showed the saved preview and `Show original file`. After scrolling the details section to its bottom and tapping the discovered button frame `(0.576, 0.900, 0.094, 0.038)` at `(0.623, 0.919)`, an alert became visible near the top: `Could not locate the memory file.` The fixture captured the plugin call with `/tmp/context-qa/synthetic-report.md`; the command failure was mapped to the visible alert and `errors` stayed empty.

Screenshots: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788962515237-21551.png`, `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788962533734-21551.png`.

### PASS — light note editable copy

The note details showed `Handwritten planning note`, `Note`, and `Use this note when explicitly selected.` `More` was discovered at `(0.580, 0.310, 0.053, 0.038)`; its menu frame was `(0.509, 0.355, 0.125, 0.088)` and `Make an editable copy` was `(0.513, 0.363, 0.115, 0.035)`. After the tap, the editor contained title `Handwritten planning note (copy)` and body `Use this note when explicitly selected.`; title input frame `(0.434, 0.358, 0.501, 0.044)` and content textarea frame `(0.434, 0.556, 0.501, 0.281)`. Fixture errors were empty.

Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788962651554-21551.png`.

### PASS — light fact editable copy

The fact details showed `Release workflow fact`, body `The release job uses the stable branch.`, `Available`, provenance `Written manually. No run citation is expected.`, source `Written by you`, and `Importance 50 / 100`. The same discovered menu frames were used. The editor contained title `Release workflow fact (copy)` and the exact original body. Fixture call: `list_memory_evidence` with `{memoryId:"context-fact"}`; errors were empty.

Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788962788881-21551.png`.

### PASS — dark report rendering

The dark report rendered its large saved preview, provenance `No verified curation citation is available for this memory.`, `Available to This workflow`, source `Workflow output`, and `Show original file`. Initial fixture calls included `list_memory_evidence` for `context-report`; errors were empty.

Screenshot: `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788962795786-21551.png`.

## History

All history states made the expected three synthetic calls: `list_run_history({workflowId:"context-qa-workflow",limit:25,offset:0})`, `get_run_history({runId:"context-qa-run"})`, and `get_memory_review_job({runId:"context-qa-run"})`; each run reported `errors:[]`.

| Theme / scenario | Visible result | Screenshot |
| --- | --- | --- |
| light / normal | `Memory context 3`; `Step 1: 1,100 of 12,000 memory bytes used`; rows `context-fact` 500 bytes rank 1 `explicit_v1`, `context-pinned` 350 bytes rank 2 `pinned_v1`, `context-ranking` 200 bytes rank 3 `lexical_v2`. | `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788962698144-21551.png` |
| dark / normal | Same 1,100/12,000 totals and row IDs/bytes; the ranking row displayed policy `lexical_v2`. | `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788962756409-21551.png` |
| light / empty | `Memory context 0`; `0 of 12,000 memory bytes used`; `Automatic recall ran and found no relevant memories`; no selected, pinned, or recalled memory recorded. | `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788962728775-21551.png` |
| dark / unavailable | `Memory context 0`; `0 of 12,000 memory bytes used`; `Automatic memory recall was unavailable`; `Some selected or pinned memory could not be recorded in the audit trace`; no audit rows. | `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788962735407-21551.png` |
| light / large | `Memory context 3`; 1,100/12,000 bytes; `3 additional relevant memories were omitted`; explicit omitted counters: `2 selected for size, 1 pinned for size, 3 recalled for size or trace availability, 1 unavailable, 2 repeated`. | `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788962741579-21551.png` |
| dark / legacy | 1,100/12,000 bytes with the same IDs/bytes; ranking row displayed policy `lexical_v1` and label `Earlier ranking record`. | `/var/folders/81/lqn73j314wndfxmq0h1f0gqm0000gn/T/argent-chromium-media/argent-screenshot-chromium-cdp-9222-1788962747744-21551.png` |

## Limits and retained first-failure history

The coordinator independently inspected the saved dark picker/retry, light fact
copy editor, dark large-report usage, and legacy History screenshots. The
exercised layouts and visible states are accepted within the fixture limits
above. This does not waive the final shared integration checks.

Before root restarted the isolated Vite listener, an earlier attempt lost the listener during the linked-memory interaction. That first failure is retained in the task history; the post-restart keyboard selection above is the bounded retest result. No claim is made that the earlier disconnected attempt proved native transport. This report does not cover Rust/backend correctness, live provider availability, real database retention, or actual Tauri IPC.
