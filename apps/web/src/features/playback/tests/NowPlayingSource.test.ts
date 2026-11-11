import { createElement } from "react";
import {
  act,
  cleanup,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { Playback } from "../playback";
import NowPlaying from "../components/NowPlaying";
import { QueueMedia, queueTrack } from "../fixtures/browserQueueMedia";
import { sourceStatusStore } from "../../sources/model/sourceStatusStore";

let player: Playback;
let media: QueueMedia;
let playbackSources: unknown[] = [];
let playbackSourceCalls: string[] = [];
const youtube = (recording: string) => ({
  id: `s-${recording}`,
  recording_id: recording,
  provider: "youtube",
  provider_label: "YouTube Music",
  label: "Opus 128 kbps",
  match_kind: "verified",
  ready: true,
  can_prepare: true,
});
const fetch = vi.fn(async (path: string) => {
  if (path.endsWith("/playback-sources")) {
    playbackSourceCalls.push(path);
    return Response.json(playbackSources);
  }
  if (path.endsWith("/lyrics/candidates")) return Response.json({ items: [] });
  if (path.endsWith("/lyrics/identities"))
    return Response.json({ recording_id: "r0", items: [] });
  return Response.json({ recording_id: "r0", items: [] });
});

vi.mock("@inertiajs/react", () => ({ router: { visit: vi.fn() } }));
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("../../artwork/components/Artwork", () => ({
  default: () => createElement("div"),
}));

beforeEach(async () => {
  sourceStatusStore.reset();
  playbackSources = [];
  playbackSourceCalls = [];
  vi.stubGlobal("fetch", fetch);
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
  media = new QueueMedia();
  player = new Playback(media, async (id) => [{ id, ready: true }]);
  await player.playQueue(
    Array.from({ length: 3 }, (_, index) =>
      queueTrack(`r${index}`, `Track ${index}`),
    ),
  );
  render(
    createElement(NowPlaying, {
      account: "alice",
      close: vi.fn(),
      openQueue: vi.fn(),
    }),
  );
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const card = () =>
  within(screen.getByRole("dialog", { name: "Now playing" })).getByRole(
    "region",
    { name: "This source" },
  );

test("both Now playing readers share one source lookup per track", async () => {
  await within(card()).findByText("This track has no playable source yet.");
  expect(
    playbackSourceCalls.filter((path) => path.includes("/recordings/r0/")),
  ).toHaveLength(1);
});

test("a source found after playback started replaces No source everywhere", async () => {
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  await within(card()).findByText("This track has no playable source yet.");
  expect(within(dialog).getAllByText("No source").length).toBeGreaterThan(0);
  playbackSources = [youtube("r0"), youtube("r1")];
  await act(async () => {
    await player.setPlaying(false);
    await player.setPlaying(true);
  });
  await waitFor(() =>
    expect(within(card()).getByText("YouTube Music")).toBeTruthy(),
  );
  expect(within(card()).getByText("Ready")).toBeTruthy();
  expect(
    within(dialog).queryByText("This track has no playable source yet."),
  ).toBeNull();
  await waitFor(() =>
    expect(within(dialog).getByText("Ready · from YouTube Music")).toBeTruthy(),
  );
});

test("a resolved source refreshes the status without a player change", async () => {
  await within(card()).findByText("This track has no playable source yet.");
  playbackSources = [youtube("r0")];
  await act(async () => {
    sourceStatusStore.invalidate("alice", "r0");
  });
  await waitFor(() =>
    expect(within(card()).getByText("YouTube Music")).toBeTruthy(),
  );
});

test("a late reply for an earlier lookup is dropped", async () => {
  await within(card()).findByText("This track has no playable source yet.");
  let release: (value: Response) => void = () => undefined;
  fetch.mockImplementationOnce(
    () => new Promise<Response>((resolve) => (release = resolve)),
  );
  await act(async () => {
    sourceStatusStore.invalidate("alice", "r0");
  });
  playbackSources = [youtube("r0")];
  await act(async () => {
    sourceStatusStore.invalidate("alice", "r0");
  });
  await waitFor(() =>
    expect(within(card()).getByText("YouTube Music")).toBeTruthy(),
  );
  await act(async () => release(Response.json([])));
  expect(within(card()).getByText("YouTube Music")).toBeTruthy();
});
