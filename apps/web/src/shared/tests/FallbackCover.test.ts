import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createElement } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import FallbackCover from "../ui/FallbackCover";

afterEach(cleanup);

test("an album cover is a quiet grey tile with a note glyph and no text", () => {
  const { container } = render(
    createElement(FallbackCover, {
      id: "r1",
      title: "Maré Baixa",
      artist: "Lume Areia",
    }),
  );
  const cover = screen.getByRole("img", { name: "Cover of Maré Baixa" });
  expect(cover.getAttribute("data-kind")).toBe("album");
  expect(cover.textContent).toBe("");
  expect(container.querySelector("svg.lucide-music")).not.toBeNull();
  expect(container.querySelector(".cover-art")).toBeNull();
  expect(cover.getAttribute("style")).toBeNull();
});

test("a playlist cover uses the list glyph and an artist cover a person, labelled as a photo", () => {
  const { container } = render(
    createElement(
      "div",
      null,
      createElement(FallbackCover, {
        id: "p1",
        title: "Drive",
        kind: "playlist",
      }),
      createElement(FallbackCover, {
        id: "a1",
        title: "Lume Areia",
        kind: "artist",
      }),
    ),
  );
  expect(container.querySelector("svg.lucide-list-music")).not.toBeNull();
  expect(container.querySelector("svg.lucide-user")).not.toBeNull();
  expect(
    screen
      .getByRole("img", { name: "Photo of Lume Areia" })
      .getAttribute("data-kind"),
  ).toBe("artist");
});

test("the liked tracks cover is a heart on the action colour", () => {
  const { container } = render(
    createElement(FallbackCover, {
      id: "liked",
      title: "Liked tracks",
      kind: "liked",
    }),
  );
  expect(
    screen
      .getByRole("img", { name: "Liked tracks cover" })
      .getAttribute("data-kind"),
  ).toBe("liked");
  expect(container.querySelector("svg.lucide-heart")).not.toBeNull();
});

test("covers of the same id render identical markup", () => {
  const props = { id: "stable", title: "Costa Norte", artist: "Lume Areia" };
  const one = render(createElement(FallbackCover, props)).container.innerHTML;
  const two = render(createElement(FallbackCover, props)).container.innerHTML;
  expect(two).toBe(one);
});

test("a genre cover still draws the generated art with its title", () => {
  const { container } = render(
    createElement(FallbackCover, { id: "fado", title: "Fado", kind: "genre" }),
  );
  expect(container.querySelector(".cover-art")).not.toBeNull();
  expect(container.querySelector(".fallback-cover-title")!.textContent).toBe(
    "Fado",
  );
  expect(container.querySelector(".fallback-cover-monogram")!.textContent).toBe(
    "F",
  );
});

test("every genre cover gets a fresh gradient id when many are on screen", () => {
  const { container } = render(
    createElement(
      "div",
      null,
      ...Array.from({ length: 40 }, (_, index) =>
        createElement(FallbackCover, {
          key: index,
          id: index % 2 ? "same" : `id-${index}`,
          title: "T",
          kind: "genre",
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

test("the quiet tile paints the neutral tile token and the glyph token", () => {
  expect(rule(".fallback-cover[data-quiet]")).toContain(
    "background: var(--cover-tile)",
  );
  expect(rule(".fallback-cover[data-quiet]")).toContain(
    "color: var(--cover-glyph)",
  );
  expect(rule('.fallback-cover[data-kind="liked"]')).toContain(
    "background: var(--action)",
  );
});

test("genre text shows from 96 px, the monogram below it, and the motif fades then disappears", () => {
  expect(css).toContain(
    "@container (width < 96px) { .fallback-cover-text { display: none; }",
  );
  expect(css).toContain(".fallback-cover-monogram { display: grid; }");
  expect(css).toContain(
    "@container (width < 48px) { .fallback-cover .cover-art-motif { display: none; }",
  );
});

test("genre titles wrap anywhere and never overflow the cover", () => {
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
