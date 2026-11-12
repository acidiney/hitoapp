import { createElement, type ReactNode } from "react";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { forgetAutomaticLookups } from "../api/automaticLyrics";
import { setAutomaticLyrics } from "../fixtures/automaticLyrics";
import { lyricRecordingPage } from "../fixtures/recordingLyrics";
import {
  automaticReadyV2,
  identities,
  plainDocument,
  readyV2,
  recordingId,
  reviewedIdentity,
  serviceA,
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
  forgetAutomaticLookups();
  cleanup();
  vi.restoreAllMocks();
  window.history.replaceState(null, "", "/");
});

const duration =
  lyricRecordingPage.detail.selected_observation?.duration_ms ?? 0;

function automaticServer(
  overrides: Parameters<typeof lyricServer>[0] = {},
  lyrics = plainDocument(["Auto found line"]),
) {
  return lyricServer({
    services: () => json(services(serviceA)),
    identities: () => json(identities()),
    automatic: () => json(automaticReadyV2(lyrics)),
    ...overrides,
  });
}

const forbidden = ["candidates", "review", "retain", "read"] as const;

describe("automatic lookup on", () => {
  setAutomaticLyrics(true);

  test("a recording without a saved choice posts exactly {} once and shows the ephemeral lyrics with their source", async () => {
    const hold = held();
    const server = automaticServer({ automatic: () => hold.reply });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    const skeleton = await lyrics.findByRole("region", {
      name: "Looking up lyrics…",
    });
    expect(skeleton.getAttribute("aria-busy")).toBe("true");
    hold.release(json(automaticReadyV2(plainDocument(["Auto found line"]))));
    expect(await lyrics.findByText("Auto found line")).toBeTruthy();
    expect(server.of("automatic")).toHaveLength(1);
    expect(server.of("automatic")[0].method).toBe("POST");
    expect(server.of("automatic")[0].body).toEqual({});
    expect(lyrics.getByText("Not synced · from Open lyrics")).toBeTruthy();
    for (const route of forbidden) expect(server.of(route)).toHaveLength(0);
  });

  test("a timed result whose length matches the audio is Synced", async () => {
    automaticServer({
      automatic: () =>
        json({
          status: "ready_v1",
          recording_id: recordingId,
          installation_id: serviceA.installation_id,
          installation_revision: serviceA.installation_revision,
          label: serviceA.label,
          provider_recording: { id: "auto-provider", revision: null },
          document_digest: "digest",
          document: {
            recording: { id: "auto-provider", revision: null },
            lyrics: {
              id: "l",
              rendition_id: "r",
              duration_ms: duration,
              lines: [
                { start_ms: 0, end_ms: 1000, text: "First timed" },
                { start_ms: 1000, end_ms: 2000, text: "Second timed" },
              ],
            },
          },
        }),
    });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    expect(await lyrics.findByText("Synced · from Open lyrics")).toBeTruthy();
  });

  test("Change on an automatic result opens Find lyrics without any search, review or save", async () => {
    const server = automaticServer();
    renderLyricsPage();
    const lyrics = openLyricsTab();
    await lyrics.findByText("Auto found line");
    fireEvent.click(lyrics.getByRole("button", { name: "Change" }));
    expect(
      await lyrics.findByRole("searchbox", { name: "Search text" }),
    ).toBeTruthy();
    for (const route of forbidden) expect(server.of(route)).toHaveLength(0);
  });

  test.each([
    ["not_found", "None found", "Find lyrics"],
    ["insufficient_metadata", "None found", "Find lyrics"],
    ["unsupported", "None found", "Find lyrics"],
    ["ambiguous", "Several matches", "Choose"],
  ] as const)("%s shows %s with %s", async (status, label, action) => {
    const server = automaticServer({ automatic: () => json({ status }) });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    expect(await lyrics.findByText(label)).toBeTruthy();
    expect(lyrics.getByRole("button", { name: action })).toBeTruthy();
    for (const route of forbidden) expect(server.of(route)).toHaveLength(0);
  });

  test("no_services points to the plugin settings and sends nothing else", async () => {
    const server = automaticServer({
      automatic: () => json({ status: "no_services" }),
    });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    expect(await lyrics.findByText("No lyrics service")).toBeTruthy();
    expect(
      lyrics
        .getByRole("link", { name: "Connect a lyrics service" })
        .getAttribute("href"),
    ).toBe("/settings?section=plugins");
    for (const route of forbidden) expect(server.of(route)).toHaveLength(0);
  });

  test("unavailable is Couldn't check with Retry, never None found, and Retry asks again", async () => {
    let answer: unknown = { status: "unavailable" };
    const server = automaticServer({ automatic: () => json(answer) });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    expect(await lyrics.findByText("Couldn't check")).toBeTruthy();
    expect(lyrics.getByText("The lyrics service didn't answer")).toBeTruthy();
    expect(lyrics.queryByText("None found")).toBeNull();
    answer = automaticReadyV2(plainDocument(["Second try"]));
    fireEvent.click(lyrics.getByRole("button", { name: "Retry" }));
    expect(await lyrics.findByText("Second try")).toBeTruthy();
    expect(server.of("automatic")).toHaveLength(2);
  });

  test("a failed request is Couldn't check with Retry", async () => {
    automaticServer({ automatic: () => json({ error: "down" }, 502) });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    expect(await lyrics.findByText("Couldn't check")).toBeTruthy();
    expect(lyrics.queryByText("None found")).toBeNull();
  });

  test("cancelled is silent: no failure and no None found", async () => {
    automaticServer({ automatic: () => json({ status: "cancelled" }) });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    expect(await lyrics.findByText("No lyrics saved")).toBeTruthy();
    expect(lyrics.queryByText("Couldn't check")).toBeNull();
    expect(lyrics.queryByText("None found")).toBeNull();
  });

  test("a ready document with no lyrics is None found", async () => {
    automaticServer({
      automatic: () => json(automaticReadyV2(null)),
    });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    expect(await lyrics.findByText("None found")).toBeTruthy();
  });

  test.each([
    [
      "another recording",
      automaticReadyV2(plainDocument(["Wrong track"]), {
        recording_id: "99999999-9999-4999-8999-999999999999",
      }),
    ],
    [
      "a document for another provider recording",
      automaticReadyV2(plainDocument(["Wrong track"]), {
        document: {
          recording: { id: "somebody-else", revision: null },
          lyrics: plainDocument(["Wrong track"]),
        },
      }),
    ],
  ])("a result for %s is rejected and never shown", async (_name, reply) => {
    const server = automaticServer({ automatic: () => json(reply) });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    expect(await lyrics.findByText("Couldn't check")).toBeTruthy();
    expect(screen.queryByText("Wrong track")).toBeNull();
    for (const route of forbidden) expect(server.of(route)).toHaveLength(0);
  });

  test("not_found is remembered for the session so reopening does not ask again", async () => {
    const server = automaticServer({
      automatic: () => json({ status: "not_found" }),
    });
    const first = renderLyricsPage();
    await openLyricsTab().findByText("None found");
    first.unmount();
    renderLyricsPage();
    expect(await openLyricsTab().findByText("None found")).toBeTruthy();
    expect(server.of("automatic")).toHaveLength(1);
  });

  test("a saved explicit choice wins and no automatic request is made", async () => {
    const saved = reviewedIdentity("88880000-0000-4000-8000-000000000001", 1);
    const server = automaticServer({
      identities: () => json(identities(saved)),
      read: () => json(readyV2(saved, plainDocument(["Saved text"]))),
    });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    expect(await lyrics.findByText("Saved text")).toBeTruthy();
    expect(server.of("automatic")).toHaveLength(0);
    const change = await openChange(lyrics);
    expect(change.getByRole("button", { name: "Find lyrics" })).toBeTruthy();
  });
});

describe("automatic lookup off", () => {
  setAutomaticLyrics(false);

  test("no automatic request is made until the user chooses to find lyrics", async () => {
    const server = automaticServer();
    renderLyricsPage();
    const lyrics = openLyricsTab();
    await waitFor(() => expect(server.of("identities")).toHaveLength(1));
    await lyrics.findByRole("button", { name: "Find lyrics" });
    expect(server.of("automatic")).toHaveLength(0);
    for (const route of forbidden) expect(server.of(route)).toHaveLength(0);
    expect(screen.queryByText("Looking up lyrics…")).toBeNull();
  });
});
