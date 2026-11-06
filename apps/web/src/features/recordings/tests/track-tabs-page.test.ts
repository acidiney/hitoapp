import { createElement, type ReactNode } from "react";
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { setAutomaticLyrics } from "../../lyrics/fixtures/automaticLyrics";
import { attributedRecordingPage } from "../fixtures/recordingAttribution";
import { renderTrackPage } from "../fixtures/trackPageView";
import { automaticChoice } from "../../sources/fixtures/sourceChoice";

setAutomaticLyrics(false);

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

afterEach(() => {
  vi.restoreAllMocks();
  window.history.replaceState(null, "", "/");
});

const tab = (name: RegExp) => screen.getByRole("tab", { name });
const selected = () =>
  screen
    .getAllByRole("tab")
    .filter((entry) => entry.getAttribute("aria-selected") === "true")
    .map((entry) => entry.textContent?.replace(/(Edited|\d+)$/, ""));

test("the page is a tablist with one selected tab, a tabpanel for each and Overview first", () => {
  renderTrackPage(attributedRecordingPage);
  expect(
    screen
      .getAllByRole("tab")
      .map((entry) => entry.textContent?.replace(/(Edited|\d+)$/, "")),
  ).toEqual(["Overview", "Lyrics", "Sources", "Metadata", "History"]);
  expect(selected()).toEqual(["Overview"]);
  const panels = document.querySelectorAll('[role="tabpanel"]');
  expect([...panels].map((panel) => panel.getAttribute("data-active"))).toEqual(
    ["true", "false", "false", "false", "false"],
  );
  for (const entry of screen.getAllByRole("tab"))
    expect(
      document
        .getElementById(entry.getAttribute("aria-controls")!)
        ?.getAttribute("aria-labelledby"),
    ).toBe(entry.id);
});

test("badges count sources once known and mark personal edits, with none on History", async () => {
  const page = structuredClone(attributedRecordingPage);
  page.detail.title.personal = "Mine";
  renderTrackPage(page, { choice: automaticChoice });
  await screen.findByRole("tab", { name: /Sources\s*2/ });
  expect(tab(/Metadata/).textContent).toContain("Edited");
  expect(tab(/History/).textContent).toBe("History");
});

test("a deep link selects its tab and an unknown one falls back to overview", () => {
  renderTrackPage(attributedRecordingPage, { search: "?tab=sources" });
  expect(selected()).toEqual(["Sources"]);
});

test("an unknown tab value opens Overview", () => {
  renderTrackPage(attributedRecordingPage, { search: "?tab=nope" });
  expect(selected()).toEqual(["Overview"]);
});

test("a block anchor opens its tab and moves focus to the block heading", async () => {
  renderTrackPage(attributedRecordingPage, { hash: "#find-audio" });
  expect(selected()).toEqual(["Sources"]);
  await waitFor(() =>
    expect(document.activeElement).toBe(document.getElementById("find-audio")),
  );
  expect(document.activeElement?.textContent).toBe("Find audio");
});

test("the legacy artist review link opens the linked artist block", async () => {
  vi.stubGlobal("fetch", vi.fn());
  renderTrackPage(attributedRecordingPage, { search: "?review=artist" });
  expect(selected()).toEqual(["Metadata"]);
  await waitFor(() =>
    expect(document.activeElement).toBe(
      document.getElementById("artist-match"),
    ),
  );
});

test("choosing a tab replaces the URL without adding a history entry", () => {
  renderTrackPage(attributedRecordingPage);
  const length = window.history.length;
  fireEvent.click(tab(/Lyrics/));
  expect(selected()).toEqual(["Lyrics"]);
  expect(window.location.search).toBe("?tab=lyrics");
  expect(window.history.length).toBe(length);
  fireEvent.click(tab(/Overview/));
  expect(window.location.search).toBe("");
});

test("arrow keys, Home and End move between tabs and activate them", () => {
  renderTrackPage(attributedRecordingPage);
  const first = tab(/Overview/);
  first.focus();
  fireEvent.keyDown(first, { key: "ArrowRight" });
  expect(selected()).toEqual(["Lyrics"]);
  expect(document.activeElement).toBe(tab(/Lyrics/));
  fireEvent.keyDown(document.activeElement!, { key: "End" });
  expect(selected()).toEqual(["History"]);
  fireEvent.keyDown(document.activeElement!, { key: "ArrowRight" });
  expect(selected()).toEqual(["Overview"]);
  fireEvent.keyDown(document.activeElement!, { key: "ArrowLeft" });
  expect(selected()).toEqual(["History"]);
  fireEvent.keyDown(document.activeElement!, { key: "Home" });
  expect(selected()).toEqual(["Overview"]);
  expect(tab(/Overview/).tabIndex).toBe(0);
  expect(tab(/Lyrics/).tabIndex).toBe(-1);
});

test("number keys choose tabs unless focus is in a text field", () => {
  renderTrackPage(attributedRecordingPage);
  fireEvent.keyDown(document.body, { key: "3" });
  expect(selected()).toEqual(["Sources"]);
  fireEvent.keyDown(document.body, { key: "5" });
  expect(selected()).toEqual(["History"]);
  fireEvent.click(tab(/Metadata/));
  fireEvent.click(screen.getByRole("button", { name: "Edit track title" }));
  const input = screen.getByRole("textbox", { name: "Track title" });
  input.focus();
  fireEvent.keyDown(input, { key: "1" });
  expect(selected()).toEqual(["Metadata"]);
});

test("Find audio in the header and in the facts open the sources tab at the block", async () => {
  renderTrackPage(attributedRecordingPage);
  const header = document.querySelector<HTMLElement>(".track-header")!;
  const primary = await within(header).findByRole("button", {
    name: "Find audio",
  });
  fireEvent.click(primary);
  expect(selected()).toEqual(["Sources"]);
  await waitFor(() =>
    expect(document.activeElement).toBe(document.getElementById("find-audio")),
  );
});

test("Copies in the rail opens the metadata tab at the copies block", async () => {
  const page = structuredClone(attributedRecordingPage);
  page.copies = {
    canonical_id: page.detail.recording_id,
    copies: [
      {
        recording_id: page.detail.recording_id,
        title: "This",
        artist: "A",
        available: true,
        canonical: true,
        proof: null,
        sources: [],
      },
      {
        recording_id: "other-copy",
        title: "Other copy",
        artist: "A",
        available: false,
        canonical: false,
        proof: "isrc",
        sources: [],
      },
    ],
  };
  renderTrackPage(page);
  fireEvent.click(screen.getByRole("button", { name: "Manage copies" }));
  expect(selected()).toEqual(["Metadata"]);
  await waitFor(() =>
    expect(document.activeElement).toBe(document.getElementById("copies")),
  );
});

test("a ready source gives the header Play and the status line names where it plays from", async () => {
  const { play } = renderTrackPage(attributedRecordingPage, {
    choice: automaticChoice,
  });
  const header = document.querySelector<HTMLElement>(".track-header")!;
  const button = await within(header).findByRole("button", { name: "Play" });
  expect(
    within(header).queryByRole("button", { name: "Find audio" }),
  ).toBeNull();
  expect(within(header).getByText("Plays from plugin")).toBeTruthy();
  expect(button.hasAttribute("disabled")).toBe(false);
  expect(play).not.toHaveBeenCalled();
});

test("a track without audio gives the header Find audio instead of Play", async () => {
  renderTrackPage(attributedRecordingPage);
  const header = document.querySelector<HTMLElement>(".track-header")!;
  await within(header).findByRole("button", { name: "Find audio" });
  expect(within(header).queryByRole("button", { name: "Play" })).toBeNull();
  expect(within(header).getByText("Catalog only")).toBeTruthy();
});
