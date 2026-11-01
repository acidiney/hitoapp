import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createElement } from "react";
import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import FallbackCover from "../ui/FallbackCover";

afterEach(cleanup);

const ids = Array.from({ length: 24 }, (_, index) => `cover-${index}`);
const variants = [
  { name: "card", wrapper: "album-card" },
  { name: "header", wrapper: "tinted-header" },
  { name: "small", wrapper: "track-cell" },
  { name: "round", wrapper: "artist-card round" },
];

test.each(variants)(
  "the $name cover prints its title on a coloured background for every id",
  ({ wrapper }) => {
    for (const id of ids) {
      const { container } = render(
        createElement(
          "div",
          { className: wrapper },
          createElement(
            "div",
            { className: "artwork" },
            createElement(FallbackCover, { id, title: `Title ${id}` }),
          ),
        ),
      );
      const cover = container.querySelector<HTMLElement>(".fallback-cover")!;
      expect(cover.textContent).toBe(`Title ${id}`);
      const background = cover.style.getPropertyValue("--cover-bg");
      expect(background).toMatch(/^#[0-9a-f]{6}$/);
      expect(["#ffffff", "#fff"]).not.toContain(background);
      expect(cover.style.getPropertyValue("--cover-fg")).toBe("#ffffff");
      cleanup();
    }
  },
);

const css = readFileSync(
  resolve(import.meta.dirname, "../../styles/primitives.css"),
  "utf8",
).replace(/\s+/g, " ");
const rule = (selector: string) => {
  const start = css.lastIndexOf(`${selector} {`);
  expect(start).toBeGreaterThan(-1);
  return css.slice(start, css.indexOf("}", start));
};

const roundCovers =
  '.artist-card.round .fallback-cover, .shelf-card[data-round] .fallback-cover, .tinted-header[data-size="artist"] .fallback-cover';

test("round covers centre the title inside the circle and drop the corner shapes", () => {
  const round = rule(roundCovers);
  expect(round).toContain("justify-content: center");
  expect(round).toContain("text-align: center");
  expect(
    rule(roundCovers.replaceAll(".fallback-cover", ".fallback-cover::before")),
  ).toContain("display: none");
});

test("the cover paints its own background so an unset colour can never leave it white", () => {
  expect(css).toMatch(/\.fallback-cover \{[^}]*background: var\(--cover-bg, #/);
});
