import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import LibrarySearch from "../LibrarySearch";

const { router } = vi.hoisted(() => ({ router: { get: vi.fn() } }));
vi.mock("@inertiajs/react", () => ({
  router,
  Link: ({ children, href, ...rest }: { children: ReactNode; href: string }) =>
    createElement("a", { href, ...rest }, children),
}));

function submit(value: string) {
  const field = screen.getByRole("textbox");
  fireEvent.change(field, { target: { value } });
  fireEvent.submit(field.closest("form") as HTMLFormElement);
}

test("the global field searches the whole app and is named plainly", () => {
  render(createElement(LibrarySearch, { query: "", target: "search" }));
  const field = screen.getByRole("textbox", { name: "Search" });
  expect(field.id).toBe("shell-search");
  expect(field.getAttribute("placeholder")).toBe(
    "Search, or paste a playlist link",
  );
  submit("tide");
  expect(router.get).toHaveBeenCalledWith("/search", { q: "tide" });
});

test("the library field filters the library and says so", () => {
  render(createElement(LibrarySearch, { query: "", target: "library" }));
  const field = screen.getByRole("textbox", { name: "Search library" });
  expect(field.getAttribute("placeholder")).toBe("Search your library");
  submit("tide");
  expect(router.get).toHaveBeenCalledWith("/library", { q: "tide" });
});

test("the field shows the query of the page it sits on", () => {
  render(createElement(LibrarySearch, { query: "glass", target: "search" }));
  expect((screen.getByRole("textbox") as HTMLInputElement).value).toBe("glass");
});

test("the top bar shows the search shortcut and the plugins pill with what needs review", () => {
  const { container } = render(
    createElement(LibrarySearch, {
      query: "",
      target: "search",
      plugins: { installed: 2, review: 1 },
    }),
  );
  expect(container.querySelector(".search kbd")?.textContent).toMatch(
    /⌘K|Ctrl K/,
  );
  const pill = screen.getByRole("link", { name: "2 plugins, 1 to review" });
  expect(pill.getAttribute("href")).toBe("/settings?section=plugins");
  expect(pill.textContent).toBe("2 plugins· 1 to review");
});

test("the plugins pill stays a plain link while the counts are unknown", () => {
  render(createElement(LibrarySearch, { query: "", target: "search" }));
  const pill = screen.getByRole("link", { name: "Plugins" });
  expect(pill.textContent).toBe("Plugins");
});
