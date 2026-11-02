import { expect, test } from "bun:test";
import { dropPointToCss } from "./use-file-drop";

test("only Windows drop positions need scaling to CSS pixels", () => {
  const point = { x: 200, y: 100 };
  expect(dropPointToCss(point, "macos", 2)).toEqual(point);
  expect(dropPointToCss(point, "linux", 2)).toEqual(point);
  expect(dropPointToCss(point, "windows", 2)).toEqual({ x: 100, y: 50 });
});
