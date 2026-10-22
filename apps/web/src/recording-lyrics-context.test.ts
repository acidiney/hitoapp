import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { ArtworkProvider } from "./ArtworkProvider";
import { NoticeProvider } from "./shared/ui/Notices";
import {
  lyricObservation,
  lyricRecordingPage,
  lyricSettings,
  readyLyrics,
} from "./fixtures/recordingLyrics";
import { automaticChoice } from "./fixtures/sourceChoice";
import RecordingDetailsPage from "./pages/RecordingDetailsPage";
import { PlaybackProvider } from "./player/PlaybackProvider";
import type { RecordingDetailsPageProps } from "./shared/types";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

function page(props: RecordingDetailsPageProps) {
  return createElement(
    NoticeProvider,
    null,
    createElement(ArtworkProvider, {
      account: props.user.id,
      children: createElement(PlaybackProvider, {
        account: props.user.id,
        children: createElement(RecordingDetailsPage, props),
      }),
    }),
  );
}

test("replacing the selected provider observation retires held lyrics and rejects a wrong response identity before retry", async () => {
  vi.spyOn(console, "info").mockImplementation(() => {});
  const errors = vi.spyOn(console, "error").mockImplementation(() => {});
  const warnings = vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const original = structuredClone(lyricRecordingPage);
  const nextObservation = {
    ...lyricObservation,
    id: "08319703-387d-4380-b5de-ae5ca59d6537",
    provider_revision: 'current-provider:"3"',
  };
  const next: RecordingDetailsPageProps = {
    ...lyricRecordingPage,
    detail: {
      ...lyricRecordingPage.detail,
      selected_observation: nextObservation,
      observations: {
        items: [nextObservation, lyricObservation],
        next_before: null,
      },
    },
  };
  const current = structuredClone(readyLyrics);
  current.observation_id = nextObservation.id;
  current.document.recording.revision = nextObservation.provider_revision;
  current.document.lyrics.lines[0].text = "Owned current observation line";
  const wrong = { ...current, observation_id: lyricObservation.id };
  let release!: (value: Response) => void;
  const held = new Promise<Response>((resolve) => {
    release = resolve;
  });
  const oldResponse = Response.json(readyLyrics);
  const base = `/api/v1/recordings/${lyricRecordingPage.detail.recording_id}`;
  let reads = 0;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === `${base}/download`)
      return Response.json({ error: "Not found" }, { status: 404 });
    if (method === "GET" && path === `${base}/source-choice`)
      return Response.json(automaticChoice);
    if (method === "GET" && path === "/api/v1/plugins")
      return Response.json(lyricSettings);
    if (method === "POST" && path === `${base}/lyrics`) {
      reads++;
      return reads === 1 ? held : Response.json(reads === 2 ? wrong : current);
    }
    throw new Error(
      "Unexpected HTTP request outside owned lyric observation fencing",
    );
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(page(lyricRecordingPage));
  try {
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
    fireEvent.click(
      within(screen.getByRole("region", { name: "Lyrics" })).getByRole(
        "button",
        { name: "Show lyrics" },
      ),
    );
    await waitFor(() => expect(reads).toBe(1));
    const oldSignal = fetch.mock.calls.find(
      ([path]) => path === `${base}/lyrics`,
    )![1]!.signal!;
    view.rerender(page(next));
    expect(oldSignal.aborted).toBe(true);
    const lyrics = within(screen.getByRole("region", { name: "Lyrics" }));
    expect(lyrics.queryByRole("status")).toBeNull();
    expect(lyrics.queryByRole("list", { name: "Lyric lines" })).toBeNull();
    fireEvent.click(lyrics.getByRole("button", { name: "Show lyrics" }));
    await lyrics.findByRole("alert");
    expect(lyrics.queryByText("Owned current observation line")).toBeNull();
    expect(lyrics.queryByRole("list", { name: "Lyric lines" })).toBeNull();
    expect(router.reload).not.toHaveBeenCalled();
    fireEvent.click(
      lyrics.getByRole("button", { name: "Refresh track details" }),
    );
    expect(router.reload).toHaveBeenCalledTimes(1);
    fireEvent.click(lyrics.getByRole("button", { name: "Retry lyrics" }));
    await lyrics.findByText("Owned current observation line");
    await act(async () => {
      release(oldResponse);
      await held;
    });
    expect(lyrics.getByText("Owned current observation line")).toBeTruthy();
    expect(
      lyrics.queryByText(readyLyrics.document.lyrics.lines[0].text),
    ).toBeNull();
    expect(lyrics.queryByRole("alert")).toBeNull();
    const commands = fetch.mock.calls.filter(
      ([, options]) => options?.method === "POST",
    );
    expect(commands.map(([path]) => path)).toEqual(
      Array(3).fill(`${base}/lyrics`),
    );
    expect(
      commands.map(([, options]) => JSON.parse(options!.body as string)),
    ).toEqual(
      [lyricObservation.id, nextObservation.id, nextObservation.id].map(
        (observation_id) => ({ observation_id, installation_revision: 9 }),
      ),
    );
    expect(fetch).toHaveBeenCalledTimes(8);
    expect(oldResponse.bodyUsed).toBe(true);
    expect(lyricRecordingPage).toEqual(original);
    expect(play).not.toHaveBeenCalled();
    expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
    expect(router.visit).not.toHaveBeenCalled();
    expect(localStorage.length + sessionStorage.length).toBe(0);
    view.unmount();
    expect(
      fetch.mock.calls.every(([, options]) => options?.signal?.aborted),
    ).toBe(true);
    expect(warnings).not.toHaveBeenCalled();
    expect(errors).not.toHaveBeenCalled();
  } finally {
    view.unmount();
    await act(async () => {
      release(oldResponse);
      await held;
    });
  }
});
