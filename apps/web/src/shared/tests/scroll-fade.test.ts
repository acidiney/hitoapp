import { createElement, useRef } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { useScrollFade } from "../hooks/useScrollFade";
import LibrarySections from "../../app/LibrarySections";

afterEach(cleanup);

function Strip() {
  const ref = useRef<HTMLDivElement>(null);
  useScrollFade(ref);
  return createElement("div", { ref, "data-testid": "strip" });
}

function measure(
  element: HTMLElement,
  sizes: { scrollWidth: number; clientWidth: number; scrollLeft: number },
) {
  for (const [name, value] of Object.entries(sizes))
    Object.defineProperty(element, name, { value, configurable: true });
  fireEvent.scroll(element);
}

test("a strip that fits shows no fade", () => {
  render(createElement(Strip));
  const strip = screen.getByTestId("strip");
  measure(strip, { scrollWidth: 300, clientWidth: 300, scrollLeft: 0 });
  expect(strip.hasAttribute("data-fade-start")).toBe(false);
  expect(strip.hasAttribute("data-fade-end")).toBe(false);
});

test("an overflowing strip fades the edge that hides more content", () => {
  render(createElement(Strip));
  const strip = screen.getByTestId("strip");
  measure(strip, { scrollWidth: 500, clientWidth: 300, scrollLeft: 0 });
  expect(strip.hasAttribute("data-fade-start")).toBe(false);
  expect(strip.hasAttribute("data-fade-end")).toBe(true);
  measure(strip, { scrollWidth: 500, clientWidth: 300, scrollLeft: 100 });
  expect(strip.hasAttribute("data-fade-start")).toBe(true);
  expect(strip.hasAttribute("data-fade-end")).toBe(true);
  measure(strip, { scrollWidth: 500, clientWidth: 300, scrollLeft: 200 });
  expect(strip.hasAttribute("data-fade-start")).toBe(true);
  expect(strip.hasAttribute("data-fade-end")).toBe(false);
});

test("the library section pills fade when they overflow", () => {
  render(createElement(LibrarySections, { path: "/library" }));
  const pills = screen.getByRole("navigation", { name: "Library sections" });
  measure(pills, { scrollWidth: 700, clientWidth: 390, scrollLeft: 0 });
  expect(pills.hasAttribute("data-fade-end")).toBe(true);
});
