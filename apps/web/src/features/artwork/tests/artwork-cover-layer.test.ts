import { createElement } from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import Artwork from "../components/Artwork";
import { ArtworkProvider } from "../components/ArtworkProvider";

beforeEach(() => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response("{}", { status: 404 })),
  );
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function mount() {
  return render(
    createElement(ArtworkProvider, {
      account: "alice",
      children: createElement(Artwork, {
        recording: "recording-1",
        fallback: { id: "group-1", title: "Aimer" },
      }),
    }),
  ).container;
}

test("a cover whose image has not arrived yet shows the generated cover instead of an empty circle", () => {
  const container = mount();
  expect(container.querySelector("img")).not.toBeNull();
  expect(container.querySelector(".fallback-cover")).not.toBeNull();
  expect(container.querySelector(".artwork")?.hasAttribute("data-loaded")).toBe(
    false,
  );
});

test("a decoded image replaces the generated cover", () => {
  const container = mount();
  fireEvent.load(container.querySelector("img")!);
  expect(container.querySelector(".fallback-cover")).toBeNull();
  expect(container.querySelector(".artwork")?.hasAttribute("data-loaded")).toBe(
    true,
  );
});

test("a failed image leaves only the generated cover", () => {
  const container = mount();
  fireEvent.error(container.querySelector("img")!);
  expect(container.querySelector("img")).toBeNull();
  expect(container.querySelectorAll(".fallback-cover")).toHaveLength(1);
});
