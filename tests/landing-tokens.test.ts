import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

// The landing page is static and has no build step, so it carries a copy of the
// design tokens. This fails when the app's tokens change and the copy does not.
test("landing page tokens match the app's tokens", () => {
  expect(readFileSync("landing/styles/tokens.css", "utf8")).toBe(
    readFileSync("src/styles/tokens.css", "utf8"),
  );
});
