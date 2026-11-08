import { createElement, type ComponentProps } from "react";
import { render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import EmptyState from "../ui/EmptyState";

vi.mock("@inertiajs/react", () => ({
  Link: (props: ComponentProps<"a">) => createElement("a", props),
}));

test("the way back to the library is a full-size button, not a small text link", () => {
  render(createElement(EmptyState, { title: "Nothing yet", text: "Empty." }));
  const link = screen.getByRole("link", { name: "Go to library" });
  expect(link.getAttribute("href")).toBe("/library");
  expect(link.classList.contains("button")).toBe(true);
  expect(link.classList.contains("text-link")).toBe(false);
});

test("the library link can be left out", () => {
  render(
    createElement(EmptyState, {
      title: "Nothing yet",
      text: "Empty.",
      libraryLink: false,
    }),
  );
  expect(screen.queryByRole("link", { name: "Go to library" })).toBeNull();
});
