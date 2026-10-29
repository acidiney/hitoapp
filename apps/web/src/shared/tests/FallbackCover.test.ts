import { createElement } from "react";
import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import FallbackCover from "../ui/FallbackCover";
import { COVER_PALETTE, coverStyle } from "../ui/coverPalette";

test("the same id always gets the same colours and shape", () => {
  const first = coverStyle("0d2f6a3e-7c2f-4a58-b1d4-1f0c2f4b8a11");
  const again = coverStyle("0d2f6a3e-7c2f-4a58-b1d4-1f0c2f4b8a11");
  expect(again).toEqual(first);
});

test("styles come from the fixed palette and spread across ids", () => {
  const ids = Array.from({ length: 200 }, (_, index) => `release-${index}`);
  const styles = ids.map(coverStyle);
  for (const style of styles) {
    expect(
      COVER_PALETTE.some((entry) => entry.background === style.background),
    ).toBe(true);
    expect(["circle", "band", "plain"]).toContain(style.shape);
  }
  expect(new Set(styles.map((style) => style.background)).size).toBe(
    COVER_PALETTE.length,
  );
  expect(new Set(styles.map((style) => style.shape)).size).toBe(3);
});

test("known ids keep their documented style", () => {
  expect(coverStyle("release-0")).toMatchInlineSnapshot(`
    {
      "accent": "#ffd9b8",
      "background": "#b5412b",
      "foreground": "#ffffff",
      "shape": "circle",
    }
  `);
});

test("the cover prints the title and artist and is labelled for assistive technology", () => {
  render(
    createElement(FallbackCover, {
      id: "r1",
      title: "Maré Baixa",
      artist: "Lume Areia",
    }),
  );
  const cover = screen.getByRole("img", { name: "Cover of Maré Baixa" });
  expect(cover.textContent).toContain("Maré Baixa");
  expect(cover.textContent).toContain("Lume Areia");
});

test("rendering twice gives identical markup", () => {
  const props = { id: "stable", title: "Costa Norte", artist: "Lume Areia" };
  const one = render(createElement(FallbackCover, props)).container.innerHTML;
  const two = render(createElement(FallbackCover, props)).container.innerHTML;
  expect(two).toBe(one);
});
