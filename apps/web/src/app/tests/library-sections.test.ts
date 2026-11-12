import { createElement } from "react";
import { render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import LibrarySections from "../LibrarySections";

vi.mock("@inertiajs/react", () => ({
  Link: ({ children, href, ...rest }: { children: string; href: string }) =>
    createElement("a", { href, ...rest }, children),
}));

const routes: [string, string][] = [
  ["/library", "Tracks"],
  ["/albums", "Albums"],
  ["/albums/abc", "Albums"],
  ["/artists", "Artists"],
  ["/playlists", "Playlists"],
  ["/podcasts", "Podcasts"],
  ["/podcasts/feed-1", "Podcasts"],
  ["/audiobooks", "Audiobooks"],
  ["/audiobooks/book-1", "Audiobooks"],
  ["/history", "History"],
];

test.each(routes)(
  "%s marks only %s as the current library tab",
  (path, label) => {
    render(createElement(LibrarySections, { path }));
    const strip = screen.getByRole("navigation", { name: "Library sections" });
    const current = within(strip)
      .getAllByRole("link")
      .filter((link) => link.getAttribute("aria-current") === "page")
      .map((link) => link.textContent);
    expect(current).toEqual([label]);
  },
);

test("the current tab is scrolled into the visible strip on load", () => {
  const scroll = vi.fn();
  const original = Object.getOwnPropertyDescriptor(
    HTMLElement.prototype,
    "scrollLeft",
  );
  Object.defineProperty(HTMLElement.prototype, "scrollLeft", {
    configurable: true,
    get: () => 0,
    set: scroll,
  });
  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    get: () => 390,
  });
  try {
    render(createElement(LibrarySections, { path: "/audiobooks" }));
    expect(scroll).toHaveBeenCalled();
  } finally {
    if (original)
      Object.defineProperty(HTMLElement.prototype, "scrollLeft", original);
    else delete (HTMLElement.prototype as { scrollLeft?: number }).scrollLeft;
    delete (HTMLElement.prototype as { clientWidth?: number }).clientWidth;
  }
});
