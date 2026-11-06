import { createElement, type ReactNode } from "react";
import { screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { setAutomaticLyrics } from "../../lyrics/fixtures/automaticLyrics";
import { attributedRecordingPage } from "../fixtures/recordingAttribution";
import { renderTrackPage } from "../fixtures/trackPageView";
import type { Schema } from "../../../shared/schema";

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

function localPage(): Schema<"RecordingDetailsPageProps"> {
  const page = structuredClone(attributedRecordingPage);
  const selected = page.detail.selected_observation!;
  selected.origin = "file_inspection";
  selected.provider = "local";
  selected.album = "Casa Aberta";
  selected.track_number = 3;
  selected.disc_number = 1;
  selected.duration_ms = 252_000;
  page.detail.audio_format = {
    format: "FLAC",
    bit_depth: 16,
    sample_rate_hz: 44100,
    size_bytes: 31_400_000,
  };
  page.detail.release_facts = { year: 2021, track_total: 11, label: "Maré" };
  page.detail.last_played = {
    played_at: Math.floor(Date.now() / 1000) - 2 * 86_400,
    position_ms: 97_000,
  };
  page.detail.appears_in = [
    { id: "pl-late", name: "Late night", kind: "local" },
    { id: "pl-road", name: "Road trip", kind: "local" },
  ];
  return page;
}

test("a ready local file shows its format and the release facts in the header and overview", () => {
  renderTrackPage(localPage());
  const header = document.querySelector(".track-header") as HTMLElement;
  expect(header.textContent).toContain("FLAC 16/44.1 · 4:12");
  expect(header.querySelector(".track-credit")?.textContent).toMatch(
    /Casa Aberta · 2021$/,
  );
  const facts = document.querySelector(".track-facts") as HTMLElement;
  expect(within(facts).getByText("Format").nextSibling?.textContent).toBe(
    "FLAC 16-bit / 44.1 kHz · 31.4 MB",
  );
  expect(within(facts).getByText("Album").nextSibling?.textContent).toBe(
    "Casa Aberta· Track 3 of 11 · Maré",
  );
  expect(within(facts).getByText("Last played").nextSibling?.textContent).toBe(
    "2 days ago · stopped at 1:37",
  );
});

test("playlists holding the track are linked in the related rail above copies", () => {
  const page = localPage();
  page.copies = {
    canonical_id: page.detail.recording_id,
    copies: [
      {
        recording_id: "other-copy",
        title: "Copy",
        artist: "A",
        album: "Deluxe",
        available: true,
        canonical: false,
        proof: "isrc",
        sources: [],
      },
    ],
  };
  renderTrackPage(page);
  const rail = screen.getByRole("complementary", { name: "Related" });
  const headings = within(rail)
    .getAllByRole("heading")
    .map((heading) => heading.textContent);
  expect(headings.slice(0, 2)).toEqual(["Appears in", "Copies"]);
  const links = within(rail)
    .getAllByRole("link", { name: /Late night|Road trip/ })
    .map((link) => link.getAttribute("href"));
  expect(links).toEqual(["/playlists/pl-late", "/playlists/pl-road"]);
});

test("nothing is invented when the server does not know a fact", () => {
  const page = structuredClone(attributedRecordingPage);
  page.detail.selected_observation!.album = "Casa Aberta";
  renderTrackPage(page);
  const header = document.querySelector(".track-header") as HTMLElement;
  expect(header.textContent).not.toMatch(/FLAC|kHz|MB/);
  const facts = document.querySelector(".track-facts") as HTMLElement;
  expect(within(facts).queryByText("Format")).toBeNull();
  expect(within(facts).queryByText("Last played")).toBeNull();
  expect(screen.queryByText("Appears in")).toBeNull();
  expect(facts.textContent).not.toMatch(/ of \d+|Maré/);
});
