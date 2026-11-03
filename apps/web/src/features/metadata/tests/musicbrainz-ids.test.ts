import { createElement, type ReactNode } from "react";
import { render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Schema } from "../../../shared/schema";
import MusicBrainzIds from "../components/MusicBrainzIds";
import { attributedRecordingPage } from "../../recordings/fixtures/recordingAttribution";
import { recordingAttributionView } from "../../recordings/fixtures/recordingAttributionView";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

const RELEASE = "0f700000-0000-4000-8000-0000000000a1";
const GROUP = "0f700000-0000-4000-8000-0000000000b1";
const RECORDING = "0f700000-0000-4000-8000-0000000000c1";

afterEach(() => vi.restoreAllMocks());

test("held identifiers are small links to musicbrainz.org that open safely", () => {
  render(
    createElement(MusicBrainzIds, {
      ids: [
        { label: "Release", kind: "release", id: RELEASE },
        { label: "Release group", kind: "release-group", id: GROUP },
      ],
    }),
  );
  const release = screen.getByRole("link", { name: RELEASE });
  expect(release.getAttribute("href")).toBe(
    `https://musicbrainz.org/release/${RELEASE}`,
  );
  expect(release.getAttribute("rel")).toBe("noreferrer noopener");
  expect(release.getAttribute("target")).toBe("_blank");
  expect(screen.getByRole("link", { name: GROUP }).getAttribute("href")).toBe(
    `https://musicbrainz.org/release-group/${GROUP}`,
  );
});

test("identifiers Hito does not hold are not shown and nothing renders when none is held", () => {
  const { container } = render(
    createElement(MusicBrainzIds, {
      ids: [
        { label: "Release", kind: "release", id: RELEASE },
        { label: "Release group", kind: "release-group", id: null },
      ],
    }),
  );
  expect(screen.queryByText("Release group")).toBeNull();
  expect(screen.getAllByRole("link")).toHaveLength(1);
  expect(container.textContent).toContain(RELEASE);
  const empty = render(
    createElement(MusicBrainzIds, {
      ids: [{ label: "Recording", kind: "recording", id: null }],
    }),
  );
  expect(empty.container.textContent).toBe("");
});

function page(id: string | null): Schema<"RecordingDetailsPageProps"> {
  return {
    ...attributedRecordingPage,
    detail: { ...attributedRecordingPage.detail, musicbrainz_recording_id: id },
  };
}

test("the recording page links its tagged recording identifier and shows nothing without one", async () => {
  const tagged = recordingAttributionView([page(RECORDING)]);
  try {
    await tagged.settled(1);
    expect(
      screen.getByRole("link", { name: RECORDING }).getAttribute("href"),
    ).toBe(`https://musicbrainz.org/recording/${RECORDING}`);
  } finally {
    tagged.finish();
  }
  const untagged = recordingAttributionView([page(null)]);
  try {
    await untagged.settled(1);
    expect(document.querySelector(".musicbrainz-ids")).toBeNull();
  } finally {
    untagged.finish();
  }
});
