import { createElement } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import LibrarySearch from "../LibrarySearch";

const { router } = vi.hoisted(() => ({ router: { get: vi.fn() } }));
vi.mock("@inertiajs/react", () => ({ router }));

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
    "Search artists, albums and tracks",
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
