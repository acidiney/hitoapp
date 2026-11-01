import { createElement, type AnchorHTMLAttributes } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import MediaCard from "../ui/MediaCard";

vi.mock("@inertiajs/react", () => ({
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

afterEach(cleanup);

const cover = createElement("span", { "data-testid": "cover" });

test("a linked card opens through one link and plays through a separate labelled button", () => {
  const onPlay = vi.fn();
  render(
    createElement(MediaCard, {
      title: "Hello",
      subtitle: "Single · 2015",
      cover,
      href: "/albums/1",
      onPlay,
    }),
  );
  const link = screen.getByRole("link", { name: /Hello/ });
  expect(link.getAttribute("href")).toBe("/albums/1");
  expect(link.textContent).toContain("Single · 2015");
  const play = screen.getByRole("button", { name: "Play Hello" });
  expect(link.contains(play)).toBe(false);
  expect(play.querySelector("a, button")).toBeNull();
  fireEvent.click(play);
  expect(onPlay).toHaveBeenCalledTimes(1);
});

test("a card without a destination opens through a button and never triggers play", () => {
  const onOpen = vi.fn();
  const onPlay = vi.fn();
  render(
    createElement(MediaCard, {
      title: "Hello",
      cover,
      openLabel: "Open Hello",
      onOpen,
      onPlay,
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Open Hello" }));
  expect(onOpen).toHaveBeenCalledTimes(1);
  expect(onPlay).not.toHaveBeenCalled();
});

test("a card without a play action has no play button", () => {
  render(createElement(MediaCard, { title: "Hello", cover, href: "/x" }));
  expect(screen.queryByRole("button")).toBeNull();
});

test("the library mark appears only when a label is given and is not interactive", () => {
  const { rerender } = render(
    createElement(MediaCard, { title: "Hello", cover, href: "/x" }),
  );
  expect(screen.queryByRole("img", { name: "In your library" })).toBeNull();
  rerender(
    createElement(MediaCard, {
      title: "Hello",
      cover,
      href: "/x",
      inLibrary: "In your library",
    }),
  );
  expect(screen.getByRole("img", { name: "In your library" })).toBeTruthy();
});

test("a heading level wraps the title inside the open control", () => {
  render(
    createElement(MediaCard, {
      title: "Hello",
      cover,
      href: "/x",
      heading: "h4",
    }),
  );
  expect(screen.getByRole("heading", { level: 4, name: "Hello" })).toBeTruthy();
});

test("a busy card disables both controls", () => {
  render(
    createElement(MediaCard, {
      title: "Hello",
      cover,
      onOpen: vi.fn(),
      onPlay: vi.fn(),
      disabled: true,
    }),
  );
  for (const button of screen.getAllByRole("button"))
    expect((button as HTMLButtonElement).disabled).toBe(true);
});
