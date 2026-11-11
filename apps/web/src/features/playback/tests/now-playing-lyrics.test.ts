import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { forgetAutoLookups } from "../../lyrics/api/autoIdentity";
import { LyricsPanel } from "../components/NowPlayingLyrics";

const apis = vi.hoisted(() => ({
  identities: vi.fn(),
  read: vi.fn(),
  services: vi.fn(),
  candidates: vi.fn(),
  review: vi.fn(),
  retain: vi.fn(),
}));
vi.mock("@inertiajs/react", () => ({ router: { visit: vi.fn() } }));
vi.mock("../../lyrics/api/lyricsApi", () => ({
  lyricsApi: {
    identities: apis.identities,
    readSelected: apis.read,
    services: apis.services,
    candidates: apis.candidates,
    review: apis.review,
    retain: apis.retain,
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
const provider = (id: string, label: string) => ({
  selectable: true,
  selector: { kind: "observation", observation_id: `obs-${id}` },
  installation_id: `inst-${id}`,
  installation_revision: 4,
  label,
  provider_recording: { id: `rec-${id}`, revision: null },
});
const listed = (...items: ReturnType<typeof provider>[]) => ({
  recording_id: "r1",
  items,
});
const ready = (id: string, text: string) => ({
  status: "ready_v1",
  recording_id: "r1",
  selector: { kind: "observation", observation_id: `obs-${id}` },
  provider_recording: { id: `rec-${id}`, revision: null },
  document_digest: "d",
  document: {
    recording: { id: `rec-${id}`, revision: null },
    lyrics: {
      id: "l",
      rendition_id: "x",
      rendition_label: null,
      lines: [{ start_ms: 0, end_ms: 1, text }],
      attribution: null,
    },
  },
});
const open = () =>
  render(
    createElement(LyricsPanel, {
      account: "alice",
      track: track as never,
      leave: vi.fn(),
    }),
  );

beforeEach(() => {
  vi.clearAllMocks();
  forgetAutoLookups();
});
afterEach(cleanup);

test("opening the tab loads lyrics from the first provider without a click", async () => {
  apis.identities.mockResolvedValue(
    listed(provider("a", "Lyrics A"), provider("b", "Lyrics B")),
  );
  apis.read.mockResolvedValue(ready("a", "First line"));
  open();
  expect(await screen.findByText("First line")).toBeTruthy();
  expect(apis.read).toHaveBeenCalledTimes(1);
  expect(apis.read.mock.calls[0][2]).toEqual({
    selector: { kind: "observation", observation_id: "obs-a" },
    installation_revision: 4,
  });
  expect(screen.queryByText(/Nothing is sent/)).toBeNull();
});

test("Change loads the lyrics of the chosen one", async () => {
  apis.identities.mockResolvedValue(
    listed(provider("a", "Lyrics A"), provider("b", "Lyrics B")),
  );
  apis.read
    .mockResolvedValueOnce(ready("a", "First line"))
    .mockResolvedValueOnce(ready("b", "Other line"));
  open();
  await screen.findByText("First line");
  fireEvent.click(screen.getByRole("button", { name: "Change" }));
  fireEvent.click(screen.getByRole("button", { name: "Lyrics B" }));
  expect(await screen.findByText("Other line")).toBeTruthy();
  expect(apis.read.mock.calls[1][2].selector).toEqual({
    kind: "observation",
    observation_id: "obs-b",
  });
});

test("without a lyrics provider the panel points to Settings plugins", async () => {
  apis.identities.mockResolvedValue(listed());
  apis.services.mockResolvedValue({ recording_id: "r1", items: [] });
  apis.services.mockResolvedValue({ recording_id: "r1", items: [] });
  open();
  expect(await screen.findByText("No lyrics provider connected")).toBeTruthy();
  expect(
    screen
      .getByRole("link", { name: "Connect one in Settings" })
      .getAttribute("href"),
  ).toBe("/settings?section=plugins");
  expect(apis.read).not.toHaveBeenCalled();
});

test("leaving the tab cancels the request in flight", async () => {
  let signal: AbortSignal | undefined;
  apis.identities.mockImplementation(
    (_a: string, _r: string, s: AbortSignal) => {
      signal = s;
      return new Promise(() => {});
    },
  );
  const view = open();
  await waitFor(() => expect(signal).toBeDefined());
  view.unmount();
  expect(signal!.aborted).toBe(true);
});

test("a failure offers a retry", async () => {
  apis.identities.mockRejectedValueOnce(new Error("down"));
  apis.identities.mockResolvedValue(listed(provider("a", "Lyrics A")));
  apis.read.mockResolvedValue(ready("a", "Back again"));
  open();
  fireEvent.click(await screen.findByRole("button", { name: "Retry" }));
  expect(await screen.findByText("Back again")).toBeTruthy();
});

test("the deadline ends in a failure with Retry, not an endless loading line", async () => {
  vi.useFakeTimers();
  apis.identities.mockImplementation(() => new Promise(() => {}));
  open();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(15_100);
  });
  expect(screen.getByText("Lyrics could not be loaded.")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Retry" })).toBeTruthy();
  vi.useRealTimers();
});

test("with automatic loading off nothing is requested until the listener asks", async () => {
  localStorage.setItem("hito:lyrics-auto:alice", "off");
  apis.identities.mockResolvedValue(listed(provider("a", "Lyrics A")));
  apis.read.mockResolvedValue(ready("a", "Asked line"));
  open();
  await Promise.resolve();
  expect(apis.identities).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Load lyrics" }));
  expect(await screen.findByText("Asked line")).toBeTruthy();
});

test("the provider is named in the footer, with whether the lines are timed", async () => {
  apis.identities.mockResolvedValue(listed(provider("a", "Lyrics A")));
  apis.read.mockResolvedValue(ready("a", "Line"));
  open();
  expect(await screen.findByText("Not synced · from Lyrics A")).toBeTruthy();
});

const readyV2 = (id: string, content: unknown) => ({
  status: "ready_v2",
  recording_id: "r1",
  selector: { kind: "observation", observation_id: `obs-${id}` },
  provider_recording: { id: `rec-${id}`, revision: null },
  document_digest: "d",
  document: {
    recording: { id: `rec-${id}`, revision: null },
    lyrics: { id: "l", rendition_id: "x", content },
  },
});

test("v2 plain and anchored lyrics show as static text with no timing buttons", async () => {
  apis.identities.mockResolvedValue(listed(provider("a", "Lyrics A")));
  apis.read.mockResolvedValue(
    readyV2("a", {
      kind: "anchored",
      lines: [
        { start_ms: 0, text: "Anchored one" },
        { start_ms: 3000, text: "" },
        { start_ms: 4000, text: "Anchored two" },
      ],
    }),
  );
  open();
  expect(await screen.findByText("Anchored one")).toBeTruthy();
  expect(screen.getByText("Anchored two")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Anchored one" })).toBeNull();
  expect(document.querySelector("[aria-current]")).toBeNull();
  expect(screen.getByText("Not synced · from Lyrics A")).toBeTruthy();
});

test("a temporary unavailable answer is a failure with a retry, never None found", async () => {
  apis.identities.mockResolvedValue(listed(provider("a", "Lyrics A")));
  apis.read.mockResolvedValue({ status: "unavailable" });
  open();
  expect(await screen.findByText("Couldn't check")).toBeTruthy();
  expect(screen.queryByText("None found")).toBeNull();
});

test("an unavailable saved identity is never read and counts as no provider when no service is connected", async () => {
  apis.identities.mockResolvedValue(
    listed({ ...provider("a", "Gone"), selectable: false }),
  );
  apis.services.mockResolvedValue({ recording_id: "r1", items: [] });
  open();
  expect(await screen.findByText("No lyrics provider connected")).toBeTruthy();
  expect(apis.read).not.toHaveBeenCalled();
});

const wanted = {
  id: "rec-found",
  title: "Song",
  artist: "Shore",
  duration_ms: 1000,
};
const lyricService = {
  recording_id: "r1",
  items: [
    { installation_id: "inst-s", installation_revision: 3, label: "Svc" },
  ],
};

function autoMocks(found: object) {
  apis.identities.mockResolvedValue(listed());
  apis.services.mockResolvedValue(lyricService);
  apis.candidates.mockResolvedValue({ items: [found] });
  apis.review.mockResolvedValue({
    candidate: { recording: wanted },
    review_stamp: "stamp",
  });
  apis.retain.mockResolvedValue({
    ...provider("found", "Svc"),
    selector: { kind: "reviewed", identity_id: "i1", identity_revision: 1 },
    installation_id: "inst-s",
    installation_revision: 3,
    provider_recording: { id: "rec-found", revision: null },
  });
  apis.read.mockResolvedValue({
    ...ready("found", "Auto line"),
    selector: { kind: "reviewed", identity_id: "i1", identity_revision: 1 },
  });
}

test("with no saved identity and the setting on, a confident match is saved and read without a click", async () => {
  autoMocks(wanted);
  open();
  expect(await screen.findByText("Auto line")).toBeTruthy();
  expect(apis.candidates.mock.calls[0][2]).toEqual({
    installation_id: "inst-s",
    installation_revision: 3,
    query: "Song Shore",
  });
  expect(apis.review.mock.calls[0][2]).toEqual({
    installation_id: "inst-s",
    installation_revision: 3,
    provider_recording_id: "rec-found",
  });
  expect(apis.retain.mock.calls[0][2]).toEqual({
    candidate: {
      installation_id: "inst-s",
      installation_revision: 3,
      provider_recording_id: "rec-found",
    },
    review_stamp: "stamp",
  });
  expect(apis.read.mock.calls[0][2].selector).toEqual({
    kind: "reviewed",
    identity_id: "i1",
    identity_revision: 1,
  });
});

test("a weak match is never reviewed or retained and None found offers Find lyrics, once per session", async () => {
  autoMocks({ ...wanted, title: "Song Remix Part 2" });
  const first = open();
  expect(await screen.findByText("None found")).toBeTruthy();
  expect(
    screen.getByRole("link", { name: "Find lyrics" }).getAttribute("href"),
  ).toContain("/recordings/r1");
  expect(apis.review).not.toHaveBeenCalled();
  expect(apis.retain).not.toHaveBeenCalled();
  first.unmount();
  open();
  await screen.findByText("None found");
  expect(apis.candidates).toHaveBeenCalledTimes(1);
});

test("a failing automatic lookup is Couldn't check and Retry runs it again", async () => {
  autoMocks(wanted);
  apis.candidates.mockRejectedValueOnce(new Error("down"));
  open();
  expect(await screen.findByText("Couldn't check")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  expect(await screen.findByText("Auto line")).toBeTruthy();
});

test("a saved identity or the setting being off never searches", async () => {
  apis.identities.mockResolvedValue(listed(provider("a", "Lyrics A")));
  apis.read.mockResolvedValue(ready("a", "Saved line"));
  const first = open();
  expect(await screen.findByText("Saved line")).toBeTruthy();
  expect(apis.candidates).not.toHaveBeenCalled();
  first.unmount();
  localStorage.setItem("hito:lyrics-auto:alice", "off");
  apis.identities.mockResolvedValue(listed());
  apis.services.mockResolvedValue(lyricService);
  open();
  fireEvent.click(await screen.findByRole("button", { name: "Load lyrics" }));
  await screen.findByText("None found");
  expect(apis.candidates).not.toHaveBeenCalled();
});
