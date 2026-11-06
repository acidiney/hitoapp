import { createElement } from "react";
import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import CoverArtSvg from "../ui/CoverArtSvg";
import { coverArt } from "../ui/coverArt";

afterEach(cleanup);

test("two instances of the same art never share gradient ids", () => {
  const art = coverArt("album", "release-1", "Casa", "Lume");
  const { container } = render(
    createElement(
      "div",
      null,
      createElement(CoverArtSvg, { art }),
      createElement(CoverArtSvg, { art }),
    ),
  );
  const ids = [...container.querySelectorAll("linearGradient")].map(
    (gradient) => gradient.id,
  );
  expect(ids).toHaveLength(2);
  expect(new Set(ids).size).toBe(2);
  for (const svg of container.querySelectorAll(".cover-art")) {
    const id = svg.querySelector("linearGradient")!.id;
    expect(svg.querySelector("rect")!.getAttribute("fill")).toBe(`url(#${id})`);
  }
});

test("the art is decoration only", () => {
  const { container } = render(
    createElement(CoverArtSvg, { art: coverArt("artist", "a", "Name", null) }),
  );
  const svg = container.querySelector("svg")!;
  expect(svg.getAttribute("aria-hidden")).toBe("true");
  expect(svg.querySelector("text")).toBeNull();
  expect(svg.querySelector("filter")).toBeNull();
  expect(svg.querySelector("radialGradient")).toBeNull();
});

test("every kind draws flat shapes inside a clipped motif stage", () => {
  for (const kind of [
    "album",
    "artist",
    "playlist",
    "podcast",
    "audiobook",
    "track",
  ] as const) {
    const { container } = render(
      createElement(CoverArtSvg, { art: coverArt(kind, "x", "Name", null) }),
    );
    const motif = container.querySelector(".cover-art-motif")!;
    expect(motif.children.length).toBeGreaterThan(0);
    cleanup();
  }
});
