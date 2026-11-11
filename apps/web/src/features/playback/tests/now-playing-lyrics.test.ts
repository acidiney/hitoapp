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
import { LyricsPanel } from "../components/NowPlayingLyrics";

const apis = vi.hoisted(() => ({
  identities: vi.fn(),
  read: vi.fn(),
  services: vi.fn(),
}));
vi.mock("@inertiajs/react", () => ({ router: { visit: vi.fn() } }));
vi.mock("../../lyrics/api/lyricsApi", () => ({
  lyricsApi: {
    identities: apis.identities,
    readSelected: apis.read,
    services: apis.services,
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

beforeEach(() => vi.clearAllMocks());
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
  fireEvent.click(await screen.findByRole("button", { name: "Try again" }));
  expect(await screen.findByText("Back again")).toBeTruthy();
});

test("the deadline ends in a failure with Try again, not an endless loading line", async () => {
  vi.useFakeTimers();
  apis.identities.mockImplementation(() => new Promise(() => {}));
  open();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(15_100);
  });
  expect(screen.getByText("Lyrics could not be loaded.")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
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
