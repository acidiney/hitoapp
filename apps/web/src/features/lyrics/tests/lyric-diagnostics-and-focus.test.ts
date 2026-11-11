import { createElement, type ReactNode } from "react";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { log } from "../../../shared/log";
import { setAutomaticLyrics } from "../fixtures/automaticLyrics";
import { lyricRecordingPage } from "../fixtures/recordingLyrics";
import {
  candidate,
  identities,
  plainDocument,
  readyV2,
  review,
  reviewedIdentity,
  serviceA,
  services,
} from "../fixtures/lyricIdentityWire";
import {
  json,
  lyricServer,
  openChange,
  openFinder,
  openLyricsTab,
  renderLyricsPage,
} from "../fixtures/lyricServer";

setAutomaticLyrics(false);

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  window.history.replaceState(null, "", "/");
});

test("lyric diagnostics carry correlation, outcome and timing but never queries, titles, lyrics or labels", async () => {
  const logged = (["debug", "info", "warn", "error"] as const).map((level) =>
    vi.spyOn(log, level),
  );
  const chosen = reviewedIdentity("66660000-0000-4000-8000-000000000001", 1);
  lyricServer({
    services: () => json(services(serviceA)),
    identities: () => json(identities(chosen)),
    candidates: () =>
      json({ items: [candidate("c1", "Secret candidate title")] }),
    review: () => json(review("c1")),
    read: () => json(readyV2(chosen, plainDocument(["Secret lyric words"]))),
  });
  renderLyricsPage();
  const lyrics = openLyricsTab();
  fireEvent.click(await lyrics.findByRole("button", { name: "Show lyrics" }));
  await lyrics.findByText("Secret lyric words");
  await openFinder(lyrics);
  fireEvent.change(lyrics.getByRole("searchbox", { name: "Search text" }), {
    target: { value: "Secret query text" },
  });
  fireEvent.click(lyrics.getByRole("button", { name: "Search" }));
  await lyrics.findByRole("button", {
    name: "Review lyrics for Secret candidate title",
  });
  const events = logged.flatMap((spy) => spy.mock.calls);
  const lyricEvents = events.filter(([event]) => event.startsWith("lyrics."));
  expect(lyricEvents.length).toBeGreaterThan(4);
  for (const [, fields] of lyricEvents)
    if (fields && "reason" in fields)
      expect(fields).toMatchObject({
        reason: expect.any(String),
        duration_ms: expect.any(Number),
      });
  const output = JSON.stringify(events);
  for (const secret of [
    "Secret query text",
    "Secret candidate title",
    "Secret lyric words",
    serviceA.label,
    lyricRecordingPage.detail.title.value,
  ])
    expect(output.includes(secret), secret).toBe(false);
});

test("the review dialog takes focus, closes on Escape and returns focus to the button that opened it", async () => {
  lyricServer({
    services: () => json(services(serviceA)),
    identities: () => json(identities()),
    candidates: () => json({ items: [candidate("c2", "Focus candidate")] }),
    review: () => json(review("c2")),
  });
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.removeAttribute("open");
  };
  renderLyricsPage();
  const lyrics = openLyricsTab();
  await openFinder(lyrics);
  fireEvent.click(lyrics.getByRole("button", { name: "Search" }));
  const opener = await lyrics.findByRole("button", {
    name: "Review lyrics for Focus candidate",
  });
  opener.focus();
  fireEvent.click(opener);
  const dialog = await screen.findByRole("dialog", {
    name: "Are these the lyrics you want?",
  });
  await waitFor(() =>
    expect(
      screen
        .getByRole("button", { name: "Use these lyrics" })
        .hasAttribute("disabled"),
    ).toBe(false),
  );
  fireEvent(dialog, new Event("cancel", { cancelable: true }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(document.activeElement).toBe(opener);
});

test("the saved choices are a keyboard radio group with visible names and the service chooser is reachable by label", async () => {
  const one = reviewedIdentity("77770000-0000-4000-8000-000000000001", 1);
  const two = reviewedIdentity("77770000-0000-4000-8000-000000000002", 1);
  lyricServer({
    services: () => json(services(serviceA)),
    identities: () => json(identities(one, two)),
  });
  renderLyricsPage();
  const lyrics = openLyricsTab();
  const radios = await (await openChange(lyrics)).findAllByRole("radio");
  expect(radios.map((radio) => radio.getAttribute("name"))).toEqual([
    "lyric-choice",
    "lyric-choice",
  ]);
  expect(screen.getByRole("group", { name: "Lyrics from" })).toBeTruthy();
  fireEvent.click(radios[1]);
  expect((radios[1] as HTMLInputElement).checked).toBe(true);
});
