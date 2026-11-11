import { createElement, type AnchorHTMLAttributes } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistCreditIdentity from "../components/ArtistCreditIdentity";
import ArtistHeader from "../components/ArtistHeader";

vi.mock("@inertiajs/react", () => ({
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
  router: { reload: vi.fn(), visit: vi.fn() },
}));

const account = "10000000-0000-4000-8000-000000000001";
const longName = Array.from({ length: 30 }, (_, i) => `Artist ${i}`).join(", ");

afterEach(() => vi.unstubAllGlobals());

test("a 300 character credit title is clamped, scaled down and kept whole for readers", () => {
  expect(longName.length).toBeGreaterThan(250);
  render(
    createElement(ArtistHeader, {
      variant: "credit",
      photo: null,
      name: longName,
      stats: "3 tracks",
    }),
  );
  const title = screen.getByRole("heading", { level: 1, name: longName });
  expect(title.getAttribute("data-size")).toBe("xl");
  expect(title.getAttribute("title")).toBe(longName);
  expect(title.textContent).toBe(longName);
});

test("short names keep the display size", () => {
  render(
    createElement(ArtistHeader, {
      variant: "credit",
      photo: null,
      name: "Ed Sheeran",
      stats: "",
    }),
  );
  expect(
    screen.getByRole("heading", { level: 1 }).getAttribute("data-size"),
  ).toBe("short");
});

test("the choose dialog uses a short search label and a truncated credit line", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string) => {
      if (path.endsWith("/match-suggestions"))
        return Response.json({
          artist_id: "credit-long",
          chosen: [],
          undecided_tracks: 3,
          dismissed: 0,
          dismissed_items: [],
          items: [],
        });
      if (path === "/api/v1/artist-matching/status")
        return Response.json({
          enabled: false,
          state: "idle",
          matched_so_far: 0,
          last_finished_at: null,
          last_matched: 0,
        });
      return Response.json({ items: [], total: 0, offset: 0, limit: 20 });
    }),
  );
  render(
    createElement(ArtistCreditIdentity, {
      account,
      artist: { id: "credit-long", name: longName },
      searchRecording: "rec-1",
    }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose the artist" }),
  );
  const search = await screen.findByRole("link", {
    name: "Search for this artist",
  });
  expect(search.textContent).not.toContain(longName);
  const actions = search.closest("footer")!;
  expect(actions.classList.contains("credit-dialog-actions")).toBe(true);
  const credit = screen.getByText(`Credit: ${longName}`);
  expect(credit.classList.contains("credit-dialog-name")).toBe(true);
  expect(credit.getAttribute("title")).toBe(longName);
});
