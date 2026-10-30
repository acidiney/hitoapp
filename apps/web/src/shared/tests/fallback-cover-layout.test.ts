import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "vitest";

const css = readFileSync(
  resolve(import.meta.dirname, "../../styles/primitives.css"),
  "utf8",
);
const rule = (selector: string) => {
  const start = css.lastIndexOf(`${selector} {`);
  expect(start).toBeGreaterThan(-1);
  return css.slice(start, css.indexOf("}", start));
};

test("the circle sits in the top-right corner above the title block", () => {
  const circle = rule('.fallback-cover[data-shape="circle"]::before');
  expect(circle).toContain("top: 9%");
  expect(circle).toContain("width: 14%");
});

test("the band is a left edge stripe that never crosses the text", () => {
  const band = rule('.fallback-cover[data-shape="band"]::before');
  expect(band).toContain("top: 0");
  expect(band).toContain("bottom: 0");
  expect(band).toContain("width: 5%");
});

test("the title is limited to two lines and the artist leaves the corner free", () => {
  expect(rule(".fallback-cover-title")).toContain("-webkit-line-clamp: 2");
  expect(rule(".fallback-cover-artist")).toContain("max-width: 74%");
});
