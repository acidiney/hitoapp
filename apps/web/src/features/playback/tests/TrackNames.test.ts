import { createElement, type AnchorHTMLAttributes } from "react";
import { render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import TrackNames from "../components/TrackNames";
import type { Schema } from "../../../shared/schema";

vi.mock("@inertiajs/react", () => ({
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

type Member = Schema<"CreditMember">;

const ada: Member = { id: "ada-1", name: "Ada", role: "main", join: " & " };
const bo: Member = { id: "bo-1", name: "Bo", role: "main" };
const cy: Member = { id: "cy-1", name: "Cy", role: "featured" };

function show(credited: Member[] | null | undefined) {
  const { container } = render(
    createElement(TrackNames, {
      artist: "Ada & Bo",
      artistHref: "/artists/combined",
      credited,
    }),
  );
  return container;
}

test("each main artist is its own link joined by the stored join text", () => {
  const container = show([ada, bo]);
  expect(
    screen.getAllByRole("link").map((link) => link.getAttribute("href")),
  ).toEqual(["/artists/ada-1", "/artists/bo-1"]);
  expect(container.textContent).toBe("Ada & Bo");
  expect(screen.queryByRole("link", { name: "Ada & Bo" })).toBeNull();
});

test("a main artist without join text falls back to a comma, with and before the last", () => {
  const container = show([
    { id: "a", name: "A", role: "main" },
    { id: "b", name: "B", role: "main" },
    { id: "c", name: "C", role: "main" },
  ]);
  expect(container.textContent).toBe("A, B & C");
});

test("featured artists follow the mains after feat. as their own links", () => {
  const container = show([{ ...ada, join: " feat. " }, cy]);
  expect(container.textContent).toBe("Ada feat. Cy");
  expect(
    screen.getAllByRole("link").map((link) => link.getAttribute("href")),
  ).toEqual(["/artists/ada-1", "/artists/cy-1"]);
});

test("several featured artists are listed after a single feat.", () => {
  const container = show([
    { ...ada, join: " & " },
    bo,
    cy,
    { id: "di-1", name: "Di", role: "featured" },
  ]);
  expect(container.textContent).toBe("Ada & Bo feat. Cy & Di");
});

test("without credited artists the plain credit keeps its single link", () => {
  for (const credited of [undefined, null, []]) {
    const { unmount } = render(
      createElement(TrackNames, {
        artist: "Ada & Bo",
        artistHref: "/artists/combined",
        credited,
      }),
    );
    expect(
      screen.getByRole("link", { name: "Ada & Bo" }).getAttribute("href"),
    ).toBe("/artists/combined");
    unmount();
  }
});
