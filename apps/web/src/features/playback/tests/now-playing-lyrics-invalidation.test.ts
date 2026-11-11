import { createElement } from "react";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { forgetAutoLookups } from "../../lyrics/api/autoIdentity";
import { LyricsPanel } from "../components/NowPlayingLyrics";
import { invalidate } from "../../../shared/invalidation";

const apis = vi.hoisted(() => ({
  identities: vi.fn(),
  read: vi.fn(),
  services: vi.fn(),
  candidates: vi.fn(),
}));
vi.mock("@inertiajs/react", () => ({ router: { visit: vi.fn() } }));
vi.mock("../../lyrics/api/lyricsApi", () => ({
  lyricsApi: {
    identities: apis.identities,
    readSelected: apis.read,
    services: apis.services,
    candidates: apis.candidates,
  },
}));

const track = {
  id: "r1",
  title: "Song",
  artist: "Shore",
  album: null,
  duration_ms: 1000,
  available: true,
};
const identity = {
  selectable: true,
  selector: { kind: "observation", observation_id: "obs-a" },
  installation_id: "inst-a",
  installation_revision: 4,
  label: "Lyrics A",
  provider_recording: { id: "rec-a", revision: null },
};
const ready = (text: string) => ({
  status: "ready_v1",
  recording_id: "r1",
  selector: identity.selector,
  provider_recording: identity.provider_recording,
  document_digest: "d",
  document: {
    recording: identity.provider_recording,
    lyrics: {
      id: "l",
      rendition_id: "x",
      rendition_label: null,
      lines: [{ start_ms: 0, end_ms: 1, text }],
      attribution: null,
    },
  },
});

beforeEach(() => {
  vi.clearAllMocks();
  forgetAutoLookups();
});
afterEach(cleanup);

test("lyrics retained on the track page appear in Now playing without reopening it", async () => {
  apis.identities.mockResolvedValueOnce({ recording_id: "r1", items: [] });
  apis.services.mockResolvedValue({ recording_id: "r1", items: [] });
  apis.candidates.mockResolvedValue({ items: [] });
  render(
    createElement(LyricsPanel, {
      account: "alice",
      track: track as never,
      leave: vi.fn(),
    }),
  );
  await screen.findByText("No lyrics provider connected");
  apis.identities.mockResolvedValue({ recording_id: "r1", items: [identity] });
  apis.read.mockResolvedValue(ready("Fresh line"));
  await act(async () => invalidate("lyrics", "r1"));
  expect(await screen.findByText("Fresh line")).toBeTruthy();
});
