import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { isFormattedBody, StepBody } from "./step-body";

test("colours JSON and shows multi-line strings as real lines", () => {
  const html = renderToStaticMarkup(
    <StepBody text={JSON.stringify({ final: true, count: 2, body: "CV ready\nPages: 2" })} />,
  );
  expect(html).toContain('<span class="json-key">&quot;final&quot;</span>');
  expect(html).toContain('<span class="json-boolean">true</span>');
  expect(html).toContain('<span class="json-number">2</span>');
  expect(html).toContain("CV ready\nPages: 2</span>");
  expect(html).not.toContain("\\n");

  const middle = renderToStaticMarkup(<StepBody text={JSON.stringify({ a: "x\ny", b: 1 })} />);
  expect(middle).toContain('x\ny</span></span><span>  <span class="json-key">');
});

test("renders agent text as markdown", () => {
  const html = renderToStaticMarkup(<StepBody text={"## Plan\n\n- **one**"} kind="prompt" />);
  expect(html).toContain("<h3>Plan</h3>");
  expect(html).toContain("<strong>one</strong>");
});

test("leaves command output raw, but still colours its JSON", () => {
  const diff = "- removed\n+ added";
  expect(renderToStaticMarkup(<StepBody text={diff} kind="git" />)).toBe(
    `<pre class="output-modal-body">${diff}</pre>`,
  );
  expect(isFormattedBody(diff, "git")).toBe(false);
  expect(isFormattedBody('{"ok":true}', "script")).toBe(true);
  expect(renderToStaticMarkup(<StepBody text={'{"ok":true}'} kind="script" />)).toContain("json-key");
});

test("does not treat broken JSON as JSON", () => {
  expect(renderToStaticMarkup(<StepBody text="{not json" />)).toContain("chat-markdown");
});
