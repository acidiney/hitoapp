import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import MemberArtistIdentity from "../components/MemberArtistIdentity";
import { artistIdentityApi } from "../api/artistIdentityApi";

vi.mock("@inertiajs/react", () => ({
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
  router: { visit: vi.fn() },
}));
vi.mock("../api/artistIdentityApi", () => ({
  artistIdentityApi: {
    memberIdentity: vi.fn(),
    applyToMemberCredits: vi.fn(),
    applyToCredit: vi.fn(),
    groupsForCredit: vi.fn(),
  },
}));

const artist = { id: "artist-guest", name: "Guest Singer" };
const group = {
  id: "group-1",
  provider: { kind: "builtin_musicbrainz" },
  provider_artist_id: "mb-guest",
  name: { value: "Guest Singer" },
  library_count: 1,
  recording_count: 1,
  release_count: 0,
};

beforeEach(() => {
  vi.mocked(artistIdentityApi.memberIdentity).mockReset();
  vi.mocked(artistIdentityApi.applyToMemberCredits).mockReset();
  vi.mocked(artistIdentityApi.applyToCredit).mockReset();
  vi.mocked(artistIdentityApi.groupsForCredit).mockReset();
});

function show() {
  render(createElement(MemberArtistIdentity, { account: "account-1", artist }));
}

test("an unidentified guest offers Choose the artist and applies to every appearance", async () => {
  vi.mocked(artistIdentityApi.memberIdentity)
    .mockResolvedValueOnce({
      artist_id: artist.id,
      chosen: [],
      undecided_tracks: 2,
    })
    .mockResolvedValue({
      artist_id: artist.id,
      chosen: [{ group_id: group.id, tracks: 2 }],
      undecided_tracks: 0,
    });
  vi.mocked(artistIdentityApi.groupsForCredit).mockResolvedValue({
    items: [group],
  } as never);
  vi.mocked(artistIdentityApi.applyToMemberCredits).mockImplementation(
    async (_account, _artist, body) => ({
      request_id: body.request_id,
      artist_id: artist.id,
      group_id: group.id,
      applied: 2,
      unchanged: 0,
      skipped_different: 0,
      skipped_unavailable: 0,
      next_after: null,
    }),
  );
  show();
  expect(await screen.findByText("Not identified yet")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Choose the artist" }));
  expect(
    await screen.findByRole("dialog", {
      name: "Which artist is Guest Singer?",
    }),
  ).toBeTruthy();
  await screen.findByRole("radio", { name: /Guest Singer/ });
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  expect(
    await screen.findByText(
      "Use Guest Singer wherever Guest Singer appears in a credit?",
    ),
  ).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  expect(
    await screen.findByText("2 appearances now use this artist."),
  ).toBeTruthy();
  expect(artistIdentityApi.applyToCredit).not.toHaveBeenCalled();
  const [, applied, body] = vi.mocked(artistIdentityApi.applyToMemberCredits)
    .mock.calls[0];
  expect(applied).toBe(artist.id);
  expect(body.group_id).toBe(group.id);
  await waitFor(() =>
    expect(screen.getByRole("link", { name: "Open artist" })).toBeTruthy(),
  );
  expect(
    screen.getByRole("link", { name: "Open artist" }).getAttribute("href"),
  ).toBe("/artist-groups/group-1");
});

test("a partly identified guest links its artist and counts the rest", async () => {
  vi.mocked(artistIdentityApi.memberIdentity).mockResolvedValue({
    artist_id: artist.id,
    chosen: [{ group_id: group.id, tracks: 1 }],
    undecided_tracks: 3,
  });
  show();
  expect(await screen.findByText(/Artist chosen/)).toBeTruthy();
  expect(screen.getByText(/3 appearances not matched yet\./)).toBeTruthy();
  expect(
    screen.getByRole("button", { name: "Choose the artist" }),
  ).toBeTruthy();
});

test("with no reviewed artist of that name the dialog explains it and only closes", async () => {
  vi.mocked(artistIdentityApi.memberIdentity).mockResolvedValue({
    artist_id: artist.id,
    chosen: [],
    undecided_tracks: 1,
  });
  vi.mocked(artistIdentityApi.groupsForCredit).mockResolvedValue({
    items: [],
  } as never);
  show();
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose the artist" }),
  );
  expect(
    await screen.findByText(
      "No artist you reviewed matches this name yet. Review it on one of the tracks it appears on first.",
    ),
  ).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Continue" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});
