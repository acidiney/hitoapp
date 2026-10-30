import { createElement, type ReactNode } from "react";
import { render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import NotFoundPage from "../NotFoundPage";
import { resolvePage } from "../pages";

vi.mock("@inertiajs/react", () => ({
  Link: ({ children, href, ...rest }: { children: ReactNode; href: string }) =>
    createElement("a", { href, ...rest }, children),
}));

test("the not found page explains itself and links back to Home and Library", () => {
  render(createElement(NotFoundPage));
  expect(screen.getByRole("heading", { name: "Page not found" })).toBeTruthy();
  expect(
    screen.getByRole("link", { name: "Go to Home" }).getAttribute("href"),
  ).toBe("/home");
  expect(
    screen
      .getByRole("link", { name: "Open your library" })
      .getAttribute("href"),
  ).toBe("/library");
});

test("a server answer naming NotFoundPage resolves inside the shell", () => {
  const page = resolvePage("NotFoundPage");
  expect(page).toBe(NotFoundPage);
  expect(page.layout).toBeTypeOf("function");
});
