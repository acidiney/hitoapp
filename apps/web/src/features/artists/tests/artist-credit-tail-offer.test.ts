import { createElement, type AnchorHTMLAttributes } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistCreditIdentity from "../components/ArtistCreditIdentity";
import type { Schema } from "../../../shared/schema";

vi.mock("@inertiajs/react", () => ({
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
  router: { reload: vi.fn(), visit: vi.fn() },
}));

const account = "10000000-0000-4000-8000-000000000001";
const credit = { id: "credit-plutonio", name: "Plutonio" };
const providerArtist = "5b11f4ce-a62d-471e-81fc-a69a8278c7da";
const suggestionsPath = `/api/v1/artists/${credit.id}/match-suggestions`;
const statusPath = "/api/v1/artist-matching/status";
const queuePath = "/api/v1/artist-matching/review-queue?limit=50";
const confirmPath = `/api/v1/artist-matching/review-queue/${credit.id}/confirm`;

const group: Schema<"ArtistIdentityGroup"> = {
  id: "70000000-0000-4000-8000-000000000011",
  provider: { kind: "builtin_musicbrainz" },
  provider_artist_id: providerArtist,
  name: {
    value: "Plutonio",
    recording_id: "r1",
    artist_id: "a1",
    association_id: "s1",
  },
  library_count: 1,
  recording_count: 5,
  release_count: 2,
};

function suggestions(undecided: number): Schema<"ArtistMatchSuggestions"> {
  return {
    artist_id: credit.id,
    chosen: [
      {
        group,
        tracks: 5,
        automatic_tracks: 5,
        evidence: [{ evidence: "matched_provider_url", tracks: 5 }],
      },
    ],
    undecided_tracks: undecided,
    dismissed: 0,
    dismissed_items: [],
    items: [],
  };
}

function queue(
  items: Schema<"ArtistReviewQueuePage">["items"],
): Schema<"ArtistReviewQueuePage"> {
  return { items, total: items.length, offset: 0, limit: 50 };
}

const offered: Schema<"ArtistReviewQueuePage">["items"][number] = {
  artist_id: credit.id,
  name: credit.name,
  undecided_tracks: 8,
  needs_search: false,
  candidates: [
    {
      name: credit.name,
      provider_artist_id: providerArtist,
      origin: "matched_on_other_tracks",
      in_library: true,
      corroborated_titles: 5,
    },
  ],
};

type Posted = { path: string; body: Record<string, unknown> };

function serve(
  reads: (() => Schema<"ArtistMatchSuggestions">)[],
  queues: (() => Schema<"ArtistReviewQueuePage">)[],
) {
  const paths: string[] = [];
  const posted: Posted[] = [];
  let read = 0;
  let listed = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, init?: RequestInit) => {
      paths.push(path);
      if (path === suggestionsPath)
        return Response.json(reads[Math.min(read++, reads.length - 1)]());
      if (path === statusPath)
        return Response.json({
          enabled: true,
          state: "idle",
          matched_so_far: 0,
          last_finished_at: null,
          last_matched: 0,
        });
      if (path === queuePath)
        return Response.json(queues[Math.min(listed++, queues.length - 1)]());
      if (path === confirmPath && init?.method === "POST") {
        const body = JSON.parse(String(init.body));
        posted.push({ path, body });
        return Response.json({
          request_id: body.request_id,
          artist_id: credit.id,
          provider_artist_id: providerArtist,
          applied: 8,
          kept: 0,
          remaining: 0,
          next_after: null,
        });
      }
      throw new Error("Unexpected request");
    }),
  );
  return { paths, posted };
}

afterEach(() => vi.unstubAllGlobals());

test("a partly matched credit offers its matched artist for the other tracks in one click", async () => {
  const { posted } = serve(
    [() => suggestions(8), () => suggestions(0)],
    [() => queue([offered]), () => queue([])],
  );
  render(createElement(ArtistCreditIdentity, { account, artist: credit }));
  const use = await screen.findByRole("button", {
    name: "Use Plutonio for the other 8 tracks",
  });
  expect(
    screen.getByRole("button", { name: "Choose a different artist" }),
  ).toBeTruthy();
  expect(
    screen.queryByRole("button", { name: "Choose the artist" }),
  ).toBeNull();
  expect(screen.queryByText(/Say which artist this credit is/)).toBeNull();
  fireEvent.click(use);
  await waitFor(() => expect(posted).toHaveLength(1));
  expect(posted[0].path).toBe(confirmPath);
  expect(posted[0].body.provider_artist_id).toBe(providerArtist);
  expect(typeof posted[0].body.request_id).toBe("string");
  expect(posted[0].body.after ?? null).toBeNull();
  await waitFor(() =>
    expect(
      screen.queryByRole("button", {
        name: "Use Plutonio for the other 8 tracks",
      }),
    ).toBeNull(),
  );
});

test("a confirmation that did not reach every track continues from where it stopped", async () => {
  let calls = 0;
  const posted: Posted[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, init?: RequestInit) => {
      if (path === suggestionsPath) return Response.json(suggestions(8));
      if (path === queuePath) return Response.json(queue([offered]));
      if (path === confirmPath) {
        const body = JSON.parse(String(init?.body));
        posted.push({ path, body });
        calls += 1;
        return Response.json({
          request_id: body.request_id,
          artist_id: credit.id,
          provider_artist_id: providerArtist,
          applied: 4,
          kept: 0,
          remaining: calls === 1 ? 4 : 0,
          next_after: calls === 1 ? "recording-4" : null,
        });
      }
      return Response.json({
        enabled: true,
        state: "idle",
        matched_so_far: 0,
        last_finished_at: null,
        last_matched: 0,
      });
    }),
  );
  render(createElement(ArtistCreditIdentity, { account, artist: credit }));
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Use Plutonio for the other 8 tracks",
    }),
  );
  await waitFor(() => expect(posted).toHaveLength(2));
  expect(posted[1].body.after).toBe("recording-4");
  expect(posted[1].body.request_id).not.toBe(posted[0].body.request_id);
});

test("a matched credit says in plain words how its tracks were matched", async () => {
  serve([() => suggestions(8)], [() => queue([offered])]);
  render(createElement(ArtistCreditIdentity, { account, artist: credit }));
  expect(
    await screen.findByText(
      "Matched on 5 tracks from the provider's artist page.",
    ),
  ).toBeTruthy();
  expect(screen.queryByText(/Matched from/)).toBeNull();
  expect(
    screen.getByText("8 tracks of this credit not matched yet."),
  ).toBeTruthy();
});

test("a partly matched credit without an offer keeps the manual choice", async () => {
  serve(
    [() => suggestions(8)],
    [() => queue([{ ...offered, artist_id: "another-credit" }])],
  );
  render(createElement(ArtistCreditIdentity, { account, artist: credit }));
  expect(
    await screen.findByRole("button", { name: "Choose the artist" }),
  ).toBeTruthy();
  expect(
    screen.getByText(
      "Say which artist this credit is, and Hito groups its tracks under that artist.",
    ),
  ).toBeTruthy();
  expect(screen.queryByRole("button", { name: /^Use Plutonio/ })).toBeNull();
});

test("an unmatched credit with no offer still shows Choose the artist and reads no queue", async () => {
  const { paths } = serve(
    [
      () => ({
        ...suggestions(13),
        chosen: [],
      }),
    ],
    [() => queue([offered])],
  );
  render(createElement(ArtistCreditIdentity, { account, artist: credit }));
  expect(await screen.findByText("Not identified yet")).toBeTruthy();
  expect(
    screen.getByRole("button", { name: "Choose the artist" }),
  ).toBeTruthy();
  expect(screen.queryByRole("button", { name: /^Use Plutonio/ })).toBeNull();
  expect(paths).not.toContain(queuePath);
});
