import { expect, test } from "vitest";
import { rule, stylesheet } from "../../test/css";

test("every album card cover is a square that keeps its ratio", () => {
  const cover = rule(stylesheet("library-pages.css"), ".album-grid .artwork");
  expect(cover).toMatch(/aspect-ratio:\s*1/);
  expect(cover).toMatch(/width:\s*100%/);
});

test("no phone rule shrinks an album card into a row or gives it a divider", () => {
  const pages = stylesheet("pages.css");
  for (const block of pages.matchAll(/([^{}]*\.album-card[^{}]*){([^}]*)}/g))
    expect(block[2]).not.toMatch(
      /height:\s*56px|border-bottom|grid-template-columns:\s*56px/,
    );
});

test("album card titles clamp to two lines and credits stay on one", () => {
  const css = stylesheet("library-pages.css");
  expect(rule(css, ".album-grid h2")).toMatch(/-webkit-line-clamp:\s*2/);
  expect(rule(css, ".album-grid small")).toMatch(/white-space:\s*nowrap/);
});
