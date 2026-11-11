import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { LyricsPanel } from "../components/NowPlayingLyrics";

const state = vi.hoisted(() => ({
  position: 0,
  listeners: new Set<() => void>(),
  seek: vi.fn(),
  identities: vi.fn(),
  read: vi.fn(),
}));
vi.mock("@inertiajs/react", () => ({ router: { visit: vi.fn() } }));
vi.mock("../components/PlaybackProvider", () => ({
  usePlayer: () => ({
    positionSnapshot: () => ({ position: state.position, duration: 10 }),
    subscribePosition: (listener: () => void) => {
      state.listeners.add(listener);
      return () => state.listeners.delete(listener);
    },
    seek: state.seek,
  }),
}));
vi.mock("../../lyrics/api/lyricsApi", () => ({
  lyricsApi: { identities: state.identities, readSelected: state.read },
}));

const track = {
  id: "r1",
  title: "Song",
  artist: "Shore",
  album: null,
  duration_ms: 10000,
  available: true,
};
const lyricsOf = (lines: [number, string][]) => ({
  status: "ready_v1",
  recording_id: "r1",
  selector: { kind: "observation", observation_id: "obs" },
  provider_recording: { id: "p", revision: null },
  document_digest: "d",
  document: {
    recording: { id: "p", revision: null },
    lyrics: {
      id: "l",
      rendition_id: "x",
      rendition_label: null,
      attribution: null,
      lines: lines.map(([start_ms, text]) => ({
        start_ms,
        end_ms: start_ms + 1000,
        text,
      })),
    },
  },
});

function setPosition(seconds: number) {
  act(() => {
    state.position = seconds;
    state.listeners.forEach((listener) => listener());
  });
}

beforeEach(() => {
  state.position = 0;
  state.identities.mockResolvedValue({
    recording_id: "r1",
    items: [
      {
        selectable: true,
        selector: { kind: "observation", observation_id: "obs" },
        installation_id: "i",
        installation_revision: 1,
        label: "Words",
        provider_recording: { id: "p", revision: null },
      },
    ],
  });
  Element.prototype.scrollIntoView = vi.fn();
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const open = () =>
  render(
    createElement(LyricsPanel, {
      account: "alice",
      track: track as never,
      leave: vi.fn(),
    }),
  );

test("timed lines follow the position and mark only the current one", async () => {
  state.read.mockResolvedValue(
    lyricsOf([
      [0, "First"],
      [2000, "Second"],
      [4000, "Third"],
    ]),
  );
  open();
  const first = await screen.findByRole("button", { name: "First" });
  expect(first.getAttribute("aria-current")).toBe("true");
  setPosition(2.5);
  expect(
    screen.getByRole("button", { name: "Second" }).getAttribute("aria-current"),
  ).toBe("true");
  expect(first.getAttribute("aria-current")).toBeNull();
  expect(first.getAttribute("data-state")).toBe("past");
  expect(
    screen.getByRole("button", { name: "Third" }).getAttribute("data-state"),
  ).toBe("next");
  expect(screen.getByText("Synced · from Words")).toBeTruthy();
});

test("a line seeks to where it starts", async () => {
  state.read.mockResolvedValue(
    lyricsOf([
      [0, "First"],
      [2000, "Second"],
    ]),
  );
  open();
  fireEvent.click(await screen.findByRole("button", { name: "Second" }));
  expect(state.seek).toHaveBeenCalledWith(2);
});

test("scrolling by hand pauses following until Back to current line", async () => {
  state.read.mockResolvedValue(
    lyricsOf([
      [0, "First"],
      [2000, "Second"],
      [4000, "Third"],
    ]),
  );
  open();
  const lines = await screen.findByRole("group", { name: "Lyric lines" });
  expect(
    screen.queryByRole("button", { name: "Back to current line" }),
  ).toBeNull();
  fireEvent.wheel(lines);
  const scroll = vi.mocked(Element.prototype.scrollIntoView);
  scroll.mockClear();
  setPosition(4.5);
  expect(scroll).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Back to current line" }));
  expect(scroll).toHaveBeenCalled();
  expect(
    screen.queryByRole("button", { name: "Back to current line" }),
  ).toBeNull();
});

test("lines without timing show at one size with no highlight and no buttons", async () => {
  state.read.mockResolvedValue(
    lyricsOf([
      [0, "Plain one"],
      [0, "Plain two"],
    ]),
  );
  open();
  expect(await screen.findByText("Plain one")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Plain one" })).toBeNull();
  expect(screen.getByText("Not synced · from Words")).toBeTruthy();
  setPosition(5);
  expect(document.querySelector("[aria-current]")).toBeNull();
});

test("while looking up, skeleton lines keep the layout and announce nothing false", () => {
  state.identities.mockReturnValue(new Promise(() => {}));
  open();
  const card = screen.getByRole("region", { name: "Looking up lyrics…" });
  expect(card.getAttribute("aria-busy")).toBe("true");
  expect(card.textContent).toContain("The music never waits for lyrics");
});
