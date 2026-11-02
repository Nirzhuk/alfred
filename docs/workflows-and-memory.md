# How workflows and memory behave

The details behind the [core ideas in the README](../README.md#core-ideas).

Use **Add step** or right-click the canvas to search for a node. Browse Essentials,
Agents, Tools, and Outputs, or type a name such as “Identity” or “Script”.

A new, untouched workflow is removed when you switch workflows, create another
one, close its tab, or navigate to another app section. Rename, edit, configure,
or explicitly save it to keep it. Existing empty workflows are not automatically
deleted.

Connect an Identity node to an agent, directly or through other steps, then
double-click it to edit its role, personality, and standing instructions. Those
instructions apply on every run to each connected downstream agent, including
custom agents. Input and Template steps can change the task text without clearing
the identity. Unconnected agents do not receive it.

Connections blend from the source node's color to the destination node's color.
Each connection handle uses its own node's color.

The **Memory library** combines search, scope filters, sorting, and an editor.
Start with a note, preference, decision, lesson, or constraint, then choose whether
it belongs to this workflow, its workspace, or all your workflows. Pin memories
for recurring context or select them in a Memories node. Expired and inactive
entries stay visible in the library and are excluded from agent context.

Open **Memory settings** in the library to control automatic recall, agent
lookup, and review suggestions. **Allow agents to look up memory** is a separate
default-off permission from Automatic recall. The current release has no
release-enabled native runtime for lookup and never falls back to Provider CLI.
Suggestions require review before becoming saved memories; the restricted
reviewer remains unavailable until its exact runtime, managed-policy, and
authorized live-activation gate passes. Linked memories keep their source, and
clearing a workflow preserves shared and linked entries.

Output save decisions belong to backend execution and use the settings captured
when the Output node runs. History shows durable receipts, including `Saved`,
`Failed`, and `Deleted`; **Retry save** retries persistence of the completed
output only and never reruns the agent. Alfred recovers pending or interrupted
saves immediately at startup in bounded batches, so a backlog drains without
an unbounded startup pass. A user/workspace memory promoted from an output
survives clearing its origin workflow.

When a verified runtime is available, on-demand lookup is read-only and exposes
exactly four bounded operations: `memory.search`, `memory.get`, `history.search`,
and `history.get`. Accepted lookup evidence is scoped to the run and records
body-free source, version, page, and delivery metadata.
