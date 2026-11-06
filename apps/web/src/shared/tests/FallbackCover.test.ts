import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createElement } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import FallbackCover from "../ui/FallbackCover";

afterEach(cleanup);

test("the cover prints the title and subtitle and is labelled for assistive technology", () => {
  render(
    createElement(FallbackCover, {
      id: "r1",
      title: "Maré Baixa",
      artist: "Lume Areia",
    }),
  );
  const cover = screen.getByRole("img", { name: "Cover of Maré Baixa" });
  expect(cover.querySelector(".fallback-cover-title")!.textContent).toBe(
    "Maré Baixa",
  );
  expect(cover.querySelector(".fallback-cover-subtitle")!.textContent).toBe(
    "Lume Areia",
  );
  expect(cover.querySelector(".fallback-cover-monogram")!.textContent).toBe(
    "M",
  );
  expect(cover.querySelector("svg")!.getAttribute("aria-hidden")).toBe("true");
});

test("an artist cover is a circle labelled as a photo", () => {
  render(
    createElement(FallbackCover, {
      id: "a1",
      title: "Lume Areia",
      kind: "artist",
    }),
  );
  const cover = screen.getByRole("img", { name: "Photo of Lume Areia" });
  expect(cover.getAttribute("data-kind")).toBe("artist");
});

test("a cover without inline text draws the art only", () => {
  const { container } = render(
    createElement(FallbackCover, { id: "r1", title: "Maré", inline: false }),
  );
  expect(container.querySelector(".fallback-cover-text")).toBeNull();
  expect(container.querySelector(".fallback-cover-monogram")).toBeNull();
  expect(container.querySelector(".cover-art")).not.toBeNull();
});

test("covers of the same id and kind render identical markup apart from gradient ids", () => {
  const props = { id: "stable", title: "Costa Norte", artist: "Lume Areia" };
  const normalise = (html: string) => html.replace(/cover-[\w-]+/g, "cover-id");
  const one = render(createElement(FallbackCover, props)).container.innerHTML;
  const two = render(createElement(FallbackCover, props)).container.innerHTML;
  expect(normalise(two)).toBe(normalise(one));
});

test("every cover gets a fresh gradient id when many are on screen", () => {
  const { container } = render(
    createElement(
      "div",
      null,
      ...Array.from({ length: 40 }, (_, index) =>
        createElement(FallbackCover, {
          key: index,
          id: index % 2 ? "same" : `id-${index}`,
          title: "T",
        }),
      ),
    ),
  );
  const ids = [...container.querySelectorAll("linearGradient")].map(
    (g) => g.id,
  );
  expect(new Set(ids).size).toBe(40);
});

const css = readFileSync(
  resolve(import.meta.dirname, "../../styles/primitives.css"),
  "utf8",
).replace(/\s+/g, " ");

function rule(selector: string) {
  const start = css.indexOf(`${selector} {`);
  expect(start).toBeGreaterThan(-1);
  return css.slice(start, css.indexOf("}", start));
}

test("text shows from 96 px, the monogram below it, and the motif fades then disappears", () => {
  expect(css).toContain(
    "@container (width < 96px) { .fallback-cover-text { display: none; }",
  );
  expect(css).toContain(".fallback-cover-monogram { display: grid; }");
  expect(css).toContain(
    "@container (width < 48px) { .fallback-cover .cover-art-motif { display: none; }",
  );
});

test("titles wrap anywhere and never overflow the cover", () => {
  expect(rule(".fallback-cover-title")).toContain("overflow-wrap: anywhere");
  expect(rule(".fallback-cover-title")).toContain("-webkit-line-clamp");
  expect(rule(".fallback-cover")).toContain("overflow: hidden");
});

test("the cover paints a base colour so an unset art can never leave it white", () => {
  expect(rule(".fallback-cover")).toMatch(/background: var\(--cover-base, #/);
});

test("the old shape styles and colour variables are gone", () => {
  expect(css).not.toContain("data-shape");
  expect(css).not.toContain("--cover-bg");
  expect(css).not.toContain("--cover-accent");
});
