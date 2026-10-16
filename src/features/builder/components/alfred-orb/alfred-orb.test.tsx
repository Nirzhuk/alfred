import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { AlfredOrb } from "./alfred-orb";

test("draws the mustache mark, hidden from assistive tech", () => {
  const html = renderToStaticMarkup(<AlfredOrb size="hero" busy />);
  expect(html).toContain('class="alfred-orb is-hero is-busy"');
  expect(html).toContain('aria-hidden="true"');
  expect(html).toContain("alfred-orb-mustache");
  expect(html).toContain('alt=""');
});

test("defaults to the small idle orb", () => {
  expect(renderToStaticMarkup(<AlfredOrb />)).toContain('class="alfred-orb is-sm"');
});
