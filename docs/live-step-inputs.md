# Live step inputs and outputs

After a workflow step starts, its canvas card shows the runtime data available
for that step:

- **Input** is the display-safe value Alfred prepared for execution. For an
  agent, this is the composed prompt after context, identity instructions,
  trigger data, and included memory have been applied.
- **Output** is the stable result produced by the step. While the step is
  running, the Output tab remains visible as `Working…`; streamed activity
  stays in the run console.

Select either tab and choose **View full input** or **View full output** to open
the complete step-data viewer. The viewer supports copying the visible value.
Agent statistics, changed files, and HTML previews remain associated with
Output.

Inputs are captured by the runner immediately before the node's main operation,
then persisted as the same value shown in History. An Input node with a
configured preparation script is the exception: the script runs first because
its result becomes part of that node's effective input.

Alfred removes credential-bearing HTTP headers and sensitive URL query values
from displayed input. A notice appears when values were redacted or when a
large input was truncated for the live desktop event.

Skipped conditional branches have no Input or Output card because those steps
were not executed.
