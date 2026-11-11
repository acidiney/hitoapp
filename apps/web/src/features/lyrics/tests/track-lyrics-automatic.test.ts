import { createElement, type ReactNode } from "react";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { forgetAutoLookups } from "../api/autoIdentity";
import { setAutomaticLyrics } from "../fixtures/automaticLyrics";
import { lyricRecordingPage } from "../fixtures/recordingLyrics";
import {
  candidate,
  review,
  identities,
  plainDocument,
  readyV2,
  reviewedIdentity,
  serviceA,
  serviceB,
  services,
} from "../fixtures/lyricIdentityWire";
import {
  held,
  json,
  lyricServer,
  openChange,
  openLyricsTab,
  renderLyricsPage,
} from "../fixtures/lyricServer";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

afterEach(() => {
  forgetAutoLookups();
  cleanup();
  vi.restoreAllMocks();
  window.history.replaceState(null, "", "/");
});

const matched = reviewedIdentity("88880000-0000-4000-8000-000000000009", 1);
const title = lyricRecordingPage.detail.title.value;
const artist = lyricRecordingPage.detail.artist.value;
const duration =
  lyricRecordingPage.detail.selected_observation?.duration_ms ?? 0;

function automaticServer(
  overrides: Parameters<typeof lyricServer>[0] = {},
  found = { id: "auto-1", title, artist, duration_ms: duration },
) {
  let saved = false;
  return lyricServer({
    services: () => json(services(serviceA, serviceB)),
    identities: () => json(identities(...(saved ? [matched] : []))),
    candidates: () =>
      json({
        items: [
          { ...candidate("weak-0", "Another song"), artist: "Nobody" },
          { ...candidate(found.id, found.title), ...found },
        ],
      }),
    review: () => json(review(found.id, "auto-stamp")),
    retain: () => {
      saved = true;
      return json(matched);
    },
    read: () => json(readyV2(matched, plainDocument(["Auto found line"]))),
    ...overrides,
  });
}

describe("automatic lookup on", () => {
  setAutomaticLyrics(true);

  test("a recording without a saved identity finds, reviews, saves and reads a confident match with the exact bodies", async () => {
    const hold = held();
    const server = automaticServer({ candidates: () => hold.reply });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    const skeleton = await lyrics.findByRole("region", {
      name: "Looking up lyrics…",
    });
    expect(skeleton.getAttribute("aria-busy")).toBe("true");
    hold.release(
      json({
        items: [
          { ...candidate("auto-1", title), artist, duration_ms: duration },
        ],
      }),
    );
    expect(await lyrics.findByText("Auto found line")).toBeTruthy();
    expect(server.of("candidates")[0].body).toEqual({
      installation_id: serviceA.installation_id,
      installation_revision: serviceA.installation_revision,
      query: `${title} ${artist}`,
    });
    const candidateBody = {
      installation_id: serviceA.installation_id,
      installation_revision: serviceA.installation_revision,
      provider_recording_id: "auto-1",
    };
    expect(server.of("review")[0].body).toEqual(candidateBody);
    expect(server.of("retain")[0].body).toEqual({
      candidate: candidateBody,
      review_stamp: "auto-stamp",
    });
    expect(server.of("read")[0].body).toEqual({
      selector: matched.selector,
      installation_revision: matched.installation_revision,
    });
    expect(lyrics.getByText(/from Open lyrics/)).toBeTruthy();
    const change = await openChange(lyrics);
    expect(
      change.getByRole<HTMLInputElement>("radio", { name: /Open lyrics/ })
        .checked,
    ).toBe(true);
    expect(change.getByRole("button", { name: "Find lyrics" })).toBeTruthy();
    expect(
      change.getByRole("button", { name: "Remove Open lyrics" }),
    ).toBeTruthy();
  });

  test("a weak match is never reviewed, retained or read and shows None found with Find lyrics", async () => {
    const server = automaticServer(
      {},
      {
        id: "weak-1",
        title: `${title} Remix Part 2`,
        artist,
        duration_ms: duration,
      },
    );
    renderLyricsPage();
    const lyrics = openLyricsTab();
    expect(await lyrics.findByText("None found")).toBeTruthy();
    expect(server.of("candidates")).toHaveLength(1);
    for (const route of ["review", "retain", "read"] as const)
      expect(server.of(route)).toHaveLength(0);
    fireEvent.click(lyrics.getByRole("button", { name: "Find lyrics" }));
    expect(
      await lyrics.findByRole("searchbox", { name: "Search text" }),
    ).toBeTruthy();
  });

  test("a wrong artist or a duration more than 2 seconds away is a weak match too", async () => {
    const server = automaticServer(
      {},
      { id: "far", title, artist, duration_ms: duration + 5000 },
    );
    renderLyricsPage();
    const lyrics = openLyricsTab();
    expect(await lyrics.findByText("None found")).toBeTruthy();
    expect(server.of("retain")).toHaveLength(0);
  });

  test("after None found the lookup does not repeat on the next open of the same recording", async () => {
    const server = automaticServer(
      {},
      { id: "weak", title: "Totally different", artist, duration_ms: duration },
    );
    const first = renderLyricsPage();
    const lyrics = openLyricsTab();
    await lyrics.findByText("None found");
    first.unmount();
    renderLyricsPage();
    const again = openLyricsTab();
    expect(await again.findByText("None found")).toBeTruthy();
    expect(server.of("candidates")).toHaveLength(1);
  });

  test("a temporary failure is Couldn't check with Retry that runs the lookup again", async () => {
    let fail = true;
    const server = automaticServer({
      candidates: () =>
        fail
          ? json({ error: "down" }, 502)
          : json({
              items: [
                {
                  ...candidate("auto-1", title),
                  artist,
                  duration_ms: duration,
                },
              ],
            }),
    });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    expect(await lyrics.findByText("Couldn't check")).toBeTruthy();
    expect(lyrics.queryByText("None found")).toBeNull();
    fail = false;
    fireEvent.click(lyrics.getByRole("button", { name: "Retry" }));
    expect(await lyrics.findByText("Auto found line")).toBeTruthy();
    expect(server.of("candidates")).toHaveLength(2);
  });

  test("no connected service is None found without any candidate request", async () => {
    const server = automaticServer({ services: () => json(services()) });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    expect(await lyrics.findByText("None found")).toBeTruthy();
    expect(server.of("candidates")).toHaveLength(0);
  });

  test("a saved identity skips the lookup and keeps the automatic read of the default selection", async () => {
    const saved = reviewedIdentity("88880000-0000-4000-8000-000000000001", 1);
    const server = automaticServer({
      identities: () => json(identities(saved)),
      read: () => json(readyV2(saved, plainDocument(["Saved text"]))),
    });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    expect(await lyrics.findByText("Saved text")).toBeTruthy();
    expect(server.of("candidates")).toHaveLength(0);
  });
});

describe("automatic lookup off", () => {
  setAutomaticLyrics(false);

  test("nothing is searched, reviewed or saved until the user chooses to find lyrics", async () => {
    const server = automaticServer();
    renderLyricsPage();
    const lyrics = openLyricsTab();
    await waitFor(() => expect(server.of("identities")).toHaveLength(1));
    await lyrics.findByRole("button", { name: "Find lyrics" });
    for (const route of ["candidates", "review", "retain", "read"] as const)
      expect(server.of(route)).toHaveLength(0);
    expect(screen.queryByText("Looking up lyrics…")).toBeNull();
  });
});
