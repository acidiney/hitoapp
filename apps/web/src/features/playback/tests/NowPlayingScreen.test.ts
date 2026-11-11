import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import { Playback } from "../playback";
import NowPlaying from "../components/NowPlaying";
import { QueueMedia, queueTrack } from "../fixtures/browserQueueMedia";

let player: Playback;
let media: QueueMedia;
const close = vi.fn();
let playbackSources: unknown[] = [];
const fetch = vi.fn(async (path: string) =>
  path.endsWith("/playback-sources")
    ? Response.json(playbackSources)
    : path.endsWith("/lyrics/identities")
      ? Response.json({ recording_id: "r0", items: [] })
      : Response.json({
          recording_id: "r0",
          items: [
            {
              installation_id: "i",
              installation_revision: 1,
              label: "Words",
            },
          ],
        }),
);
const openQueue = vi.fn();
vi.mock("@inertiajs/react", () => ({ router: { visit: vi.fn() } }));
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("../../artwork/components/Artwork", () => ({
  default: () => createElement("div", { "data-testid": "artwork" }),
}));

beforeEach(async () => {
  playbackSources = [
    {
      id: "s0",
      recording_id: "r0",
      provider: "youtube",
      provider_label: "YouTube Music",
      label: "Opus 128 kbps",
      match_kind: "verified",
      ready: true,
      can_prepare: true,
    },
  ];
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
    Array.from({ length: 8 }, (_, index) =>
      queueTrack(`r${index}`, `Track ${index}`),
    ),
  );
  render(createElement(NowPlaying, { account: "alice", close, openQueue }));
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  close.mockClear();
  openQueue.mockClear();
});

test("the full-screen player shows the track, transport, lyrics, up next and source together", () => {
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  expect(
    within(dialog).getByRole("heading", { name: "Track 0", level: 1 }),
  ).toBeTruthy();
  expect(within(dialog).getByRole("slider", { name: "Seek" })).toBeTruthy();
  for (const name of ["Shuffle", "Previous track", "Pause", "Next track"])
    expect(within(dialog).getByRole("button", { name })).toBeTruthy();
  expect(within(dialog).queryByRole("tablist")).toBeNull();
  for (const name of ["Lyrics", "Up next", "This source", "Speed"])
    expect(within(dialog).getByRole("region", { name })).toBeTruthy();
});

test("Up next shows the next queued track and opens the full queue", async () => {
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  await waitFor(() => expect(within(dialog).getByText("Track 1")).toBeTruthy());
  expect(within(dialog).queryByText("Track 2")).toBeNull();
  fireEvent.click(within(dialog).getByRole("link", { name: "Open queue" }));
  expect(openQueue).toHaveBeenCalledTimes(1);
});

test("Lyrics asks the account for a lyrics provider instead of waiting for a click", async () => {
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  expect(
    await within(dialog).findByText("No lyrics found for this track."),
  ).toBeTruthy();
  expect(
    within(dialog).getByRole("link", { name: "Track details" }),
  ).toHaveProperty("pathname", "/recordings/r0");
});

test("Source shows the speed the source plays at and sets the speed", () => {
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  const speed = within(dialog).getByRole("radiogroup", { name: "Speed" });
  expect(within(speed).getByRole("radio", { name: "1×" })).toHaveProperty(
    "ariaChecked",
    "true",
  );
  vi.useFakeTimers();
  fireEvent.click(within(speed).getByRole("radio", { name: "1.5×" }));
  act(() => vi.advanceTimersByTime(400));
  vi.useRealTimers();
  expect(media.playbackRate).toBe(1.5);
});

test("Source names the service and its state in plain words", async () => {
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  const card = within(dialog).getByRole("region", { name: "This source" });
  expect(await within(card).findByText("YouTube Music")).toBeTruthy();
  expect(within(card).getByText("Ready")).toBeTruthy();
  expect(within(card).queryByText("Best guess")).toBeNull();
  expect(card.textContent).not.toMatch(/player|This browser|speeds/);
});

test("Source marks a best guess", async () => {
  playbackSources = [
    {
      ...(playbackSources[0] as object),
      match_kind: "best_guess",
    },
  ];
  cleanup();
  render(createElement(NowPlaying, { account: "alice", close, openQueue }));
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  expect(await within(dialog).findByText("Best guess")).toBeTruthy();
});

test("Change source opens the Sources tab of the track and closes the player", async () => {
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Change source" }),
  );
  expect(close).toHaveBeenCalledTimes(1);
  expect(vi.mocked(router.visit).mock.calls.at(-1)?.[0]).toBe(
    "/recordings/r0?tab=sources#find-audio",
  );
});

test("rapid speed choices coalesce into one command", () => {
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  const speed = within(dialog).getByRole("radiogroup", { name: "Speed" });
  const command = vi.spyOn(player, "speed");
  vi.useFakeTimers();
  for (const name of ["1.25×", "1.5×", "2×"])
    fireEvent.click(within(speed).getByRole("radio", { name }));
  expect(command).not.toHaveBeenCalled();
  expect(
    within(speed)
      .getByRole("radio", { name: "2×" })
      .getAttribute("aria-checked"),
  ).toBe("true");
  act(() => vi.advanceTimersByTime(400));
  vi.useRealTimers();
  expect(command).toHaveBeenCalledTimes(1);
  expect(command).toHaveBeenCalledWith(2);
});

test("following a link inside the player closes it and then focuses the new page", async () => {
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  fireEvent.click(
    within(dialog).getByRole("link", { name: "Owned performer" }),
  );
  expect(close).toHaveBeenCalledTimes(1);
  fireEvent.click(
    await within(dialog).findByRole("link", { name: "Track details" }),
  );
  expect(close).toHaveBeenCalledTimes(2);
  const visit = vi.mocked(router.visit);
  const options = visit.mock.calls.at(-1)?.[1] as { onSuccess(): void };
  document.body.insertAdjacentHTML(
    "beforeend",
    '<main><h1 id="landing">Details</h1></main>',
  );
  vi.useFakeTimers();
  options.onSuccess();
  vi.runAllTimers();
  vi.useRealTimers();
  expect(document.activeElement?.id).toBe("landing");
});

test("position ticks do not fetch queue pages", async () => {
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  await waitFor(() => expect(within(dialog).getByText("Track 1")).toBeTruthy());
  const pages = vi.spyOn(player, "queuePage");
  for (let tick = 1; tick <= 5; tick++)
    act(() => {
      media.currentTime = tick;
      media.dispatchEvent(new Event("timeupdate"));
    });
  expect(pages).not.toHaveBeenCalled();
  expect(within(dialog).getByRole("slider", { name: "Seek" })).toHaveProperty(
    "value",
    "5",
  );
});

test("the close button closes the dialog", () => {
  fireEvent.click(screen.getByRole("button", { name: "Close now playing" }));
  expect(close).toHaveBeenCalledTimes(1);
});
