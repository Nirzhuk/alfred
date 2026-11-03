# Live step inputs and outputs

After a workflow step starts, its canvas card shows the runtime data available
for that step:

- **Input** is the display-safe value Alfred prepared for execution. For an
  agent, this is the composed prompt after context, identity instructions,
  trigger data, and included memory have been applied.
- **Output** is the stable result produced by the step. While the step is
  running, the Output tab remains visible as `Working…`; streamed activity
  stays in the run console.

A finished step opens on Output. Input, Memory, and File inject steps open on
Input, because they supply data rather than transform it.

Files the visible tab mentions appear on the card. Up to two show by name and
open directly; more show as a count that opens the viewer.

Select either tab and choose **View full input** or **View full output** to open
the complete step-data viewer. The viewer supports copying the visible value.
Agent statistics, changed files, and HTML previews remain associated with
Output.

The viewer colours JSON and renders prompts and agent text as markdown. Script,
git, and HTTP text stays raw so its alignment survives. **View source** shows
the original text, which is also what **Copy** copies.

Inputs are captured by the runner immediately before the node's main operation,
then persisted as the same value shown in History. An Input node with a
configured preparation script is the exception: the script runs first because
its result becomes part of that node's effective input.

Alfred removes credential-bearing HTTP headers and sensitive URL query values
from displayed input. A notice appears when values were redacted or when a
large input was truncated for the live desktop event.

## Files on input steps

Add files by dropping them on an Input or File inject card, or on the step's
settings. The **+ File** and **+ Folder** buttons still work. A folder adds its
path and top-level listing to the context, not the contents of its files.

Select a file on the card, or **Edit** in settings and in the viewer, to edit it
in Alfred. **Save** (Cmd/Ctrl+S) writes to the original file, so the next run
uses the new contents. If the file changed on disk after you opened it, Alfred
asks whether to reload or overwrite. Binary, non-UTF-8, and over-2 MB files open
with **Open in default app** instead.

## Skipped steps

Skipped conditional branches have no Input or Output card because those steps
were not executed.
