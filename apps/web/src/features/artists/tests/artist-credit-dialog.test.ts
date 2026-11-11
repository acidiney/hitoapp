import { createElement, type AnchorHTMLAttributes } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistCreditIdentity from "../components/ArtistCreditIdentity";

vi.mock("@inertiajs/react", () => ({
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
  router: { reload: vi.fn(), visit: vi.fn() },
}));

const account = "10000000-0000-4000-8000-000000000001";
const credit = { id: "credit-ed", name: "Ed Sheeran" };

function serve() {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string) => {
      if (path === `/api/v1/artists/${credit.id}/match-suggestions`)
        return Response.json({
          artist_id: credit.id,
          chosen: [],
          undecided_tracks: 3,
          dismissed: 0,
          dismissed_items: [],
          items: [],
        });
      if (path === "/api/v1/artist-matching/status")
        return Response.json({
          enabled: true,
          state: "idle",
          matched_so_far: 0,
          last_finished_at: null,
          last_matched: 0,
        });
      if (path.startsWith("/api/v1/artist-identity-groups"))
        return Response.json({ items: [], total: 0, offset: 0, limit: 20 });
      throw new Error(`Unexpected ${path}`);
    }),
  );
}

afterEach(() => vi.unstubAllGlobals());

test("an unidentified credit shows a short sentence and a button, not a wall of text", async () => {
  serve();
  render(
    createElement(ArtistCreditIdentity, {
      account,
      artist: credit,
      searchRecording: "rec-1",
    }),
  );
  await screen.findByRole("button", { name: "Choose the artist" });
  expect(screen.queryByText(/Equal names alone/)).toBeNull();
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("the choice opens in a dialog with a search link when no reviewed artist matches, and focus returns on close", async () => {
  serve();
  render(
    createElement(ArtistCreditIdentity, {
      account,
      artist: credit,
      searchRecording: "rec 1",
    }),
  );
  const button = await screen.findByRole("button", {
    name: "Choose the artist",
  });
  button.focus();
  fireEvent.click(button);
  const dialog = await screen.findByRole("dialog", {
    name: "Choose the artist",
  });
  await within(dialog).findByText(/No artist you reviewed matches/);
  expect(
    within(dialog)
      .getByRole("link", { name: "Search for Ed Sheeran" })
      .getAttribute("href"),
  ).toBe("/recordings/rec%201?review=artist");
  fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(document.activeElement).toBe(button);
});
