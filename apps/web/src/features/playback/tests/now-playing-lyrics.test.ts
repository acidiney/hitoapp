import { createElement } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { LyricsPanel } from "../components/NowPlayingPanels";

const apis = vi.hoisted(() => ({
  providers: vi.fn(),
  read: vi.fn(),
  settings: vi.fn(),
}));
vi.mock("@inertiajs/react", () => ({ router: { visit: vi.fn() } }));
vi.mock("../../lyrics/api/lyricsApi", () => ({
  lyricsApi: { providers: apis.providers, read: apis.read },
}));
vi.mock("../../plugins/api/pluginsApi", () => ({
  pluginsApi: { settings: apis.settings },
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
  installation_id: `inst-${id}`,
  installation_revision: 4,
  label,
  observation_id: `obs-${id}`,
  provider_recording: { id: `rec-${id}`, revision: null },
});
const ready = (id: string, text: string) => ({
  status: "ready",
  recording_id: "r1",
  observation_id: `obs-${id}`,
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
  apis.providers.mockResolvedValue({
    recording_id: "r1",
    items: [provider("a", "Lyrics A"), provider("b", "Lyrics B")],
  });
  apis.read.mockResolvedValue(ready("a", "First line"));
  open();
  expect(await screen.findByText("First line")).toBeTruthy();
  expect(apis.read).toHaveBeenCalledTimes(1);
  expect(apis.read.mock.calls[0][2]).toEqual({
    observation_id: "obs-a",
    installation_revision: 4,
  });
  expect(screen.queryByText(/Nothing is sent/)).toBeNull();
});

test("Choose another provider loads the lyrics of the chosen one", async () => {
  apis.providers.mockResolvedValue({
    recording_id: "r1",
    items: [provider("a", "Lyrics A"), provider("b", "Lyrics B")],
  });
  apis.read
    .mockResolvedValueOnce(ready("a", "First line"))
    .mockResolvedValueOnce(ready("b", "Other line"));
  open();
  await screen.findByText("First line");
  fireEvent.click(screen.getByRole("button", { name: "Choose another provider" }));
  fireEvent.click(screen.getByRole("button", { name: "Lyrics B" }));
  expect(await screen.findByText("Other line")).toBeTruthy();
  expect(apis.read.mock.calls[1][2].observation_id).toBe("obs-b");
});

test("without a lyrics provider the panel points to Settings plugins", async () => {
  apis.providers.mockResolvedValue({ recording_id: "r1", items: [] });
  apis.settings.mockResolvedValue({ installations: [], services: [] });
  open();
  expect(await screen.findByText("No lyrics provider connected")).toBeTruthy();
  expect(
    screen.getByRole("link", { name: "Connect one in Settings" }).getAttribute("href"),
  ).toBe("/settings?section=plugins");
  expect(apis.read).not.toHaveBeenCalled();
});

test("leaving the tab cancels the request in flight", async () => {
  let signal: AbortSignal | undefined;
  apis.providers.mockImplementation(
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
  apis.providers.mockRejectedValueOnce(new Error("down"));
  apis.providers.mockResolvedValue({
    recording_id: "r1",
    items: [provider("a", "Lyrics A")],
  });
  apis.read.mockResolvedValue(ready("a", "Back again"));
  open();
  fireEvent.click(await screen.findByRole("button", { name: "Try again" }));
  expect(await screen.findByText("Back again")).toBeTruthy();
});
