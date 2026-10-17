// @vitest-environment jsdom
import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import RecordingDetailsPage from "./pages/RecordingDetailsPage";
import { NoticeProvider } from "./components/Notices";
import { PlaybackProvider, usePlayer } from "./player/PlaybackProvider";
import type { Player } from "./player/model";
import type { RecordingDetailsPageProps } from "./types";
import { recordingB } from "./fixtures/playlistPages";
import {
  attachedPage,
  attachedRecording,
  attachedSources,
  attachmentCandidate,
  attachmentReview,
  attachmentSearch,
  attachmentSettings,
  reviewedAttachment,
  savedRecording,
  sourceAttachmentPage,
} from "./fixtures/sourceAttachment";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
  localStorage.clear();
});

test("a saved provider track reviews an explicit source match and keeps its unsaved personal draft", async () => {
  const errors = vi.spyOn(console, "error");
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi
    .spyOn(HTMLMediaElement.prototype, "play")
    .mockImplementation(async function (this: HTMLMediaElement) {
      this.dispatchEvent(new Event("play"));
    });
  const base = `/api/v1/recordings/${savedRecording.id}/sources`;
  const fetch = vi.fn(async (path: string, _options?: RequestInit) => {
    const responses: Record<string, unknown> = {
      "/api/v1/plugins": attachmentSettings,
      "/api/v1/plugins/video-connection/search": attachmentSearch,
      [`${base}/review`]: attachmentReview,
      [`${base}/attach`]: attachedRecording,
      [`${base}/refresh`]: attachedSources,
      [base]: attachedSources,
    };
    if (!(path in responses))
      throw new Error(
        "Unexpected request outside the owned attachment workflow",
      );
    return new Response(JSON.stringify(responses[path]));
  });
  vi.stubGlobal("fetch", fetch);
  let player!: Player;
  function Page(props: RecordingDetailsPageProps) {
    player = usePlayer();
    return createElement(RecordingDetailsPage, props);
  }
  const view = (props: RecordingDetailsPageProps) =>
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: props.user.id,
        children: createElement(Page, props),
      }),
    );
  const { rerender, unmount } = render(view(sourceAttachmentPage));
  const draft = screen.getByRole("textbox", {
    name: "Track title",
  }) as HTMLInputElement;
  fireEvent.change(draft, { target: { value: "Unsent personal title" } });
  expect(fetch).not.toHaveBeenCalled();
  expect(play).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Find a source" }));
  const service = await screen.findByRole("combobox", {
    name: "Music service",
  });
  expect((service as HTMLSelectElement).value).toBe("video-connection");
  expect(screen.getByRole("option", { name: /Video archive/ })).toBeTruthy();
  expect(
    screen.queryByRole("option", {
      name: /Search catalogue|Metadata catalogue/,
    }),
  ).toBeNull();
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(fetch.mock.calls[0][0]).toBe("/api/v1/plugins");
  const search = screen.getByRole("searchbox", { name: "Search music" });
  fireEvent.change(search, { target: { value: "Owned track audio" } });
  fireEvent.click(screen.getByRole("button", { name: "Search" }));
  const reviewButton = await screen.findByRole("button", {
    name: "Review match",
  });
  expect(screen.getByText("Search result title")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Use this source" })).toBeNull();
  expect(fetch.mock.calls.map(([path]) => path)).toEqual([
    "/api/v1/plugins",
    "/api/v1/plugins/video-connection/search",
  ]);
  expect(JSON.parse(fetch.mock.calls[1][1]?.body as string)).toEqual({
    installation_revision: 7,
    query: "Owned track audio",
    cursor: null,
    limit: 20,
  });
  fireEvent.click(reviewButton);
  const comparison = await screen.findByRole("region", {
    name: "Review source match",
  });
  expect(within(comparison).getByText(savedRecording.title)).toBeTruthy();
  expect(within(comparison).getByText(savedRecording.artist)).toBeTruthy();
  expect(within(comparison).getByText("2:00")).toBeTruthy();
  expect(
    within(comparison).getByText(attachmentReview.candidate.title),
  ).toBeTruthy();
  expect(
    within(comparison).getByText(attachmentReview.candidate.artist),
  ).toBeTruthy();
  expect(within(comparison).getByText("2:04")).toBeTruthy();
  expect(within(comparison).queryByText("Search result title")).toBeNull();
  expect(within(comparison).queryByText("Unsent personal title")).toBeNull();
  expect(fetch.mock.calls[2][0]).toBe(`${base}/review`);
  expect(JSON.parse(fetch.mock.calls[2][1]?.body as string)).toEqual(
    attachmentCandidate,
  );
  expect(fetch).toHaveBeenCalledTimes(3);
  expect(player.queueSnapshot().total).toBe(0);
  expect(play).not.toHaveBeenCalled();
  expect(draft.value).toBe("Unsent personal title");
  fireEvent.click(
    within(comparison).getByRole("button", { name: "Use this source" }),
  );
  await screen.findByText("Source added to this track.");
  expect(fetch.mock.calls[3][0]).toBe(`${base}/attach`);
  expect(JSON.parse(fetch.mock.calls[3][1]?.body as string)).toEqual(
    reviewedAttachment,
  );
  expect(fetch).toHaveBeenCalledTimes(4);
  expect(player.queueSnapshot().total).toBe(0);
  expect(play).not.toHaveBeenCalled();
  rerender(view(attachedPage));
  expect(screen.getByRole("textbox", { name: "Track title" })).toBe(draft);
  expect(draft.value).toBe("Unsent personal title");
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
    savedRecording.title,
  );
  expect(
    (screen.getByRole("textbox", { name: "Track artist" }) as HTMLInputElement)
      .value,
  ).toBe(savedRecording.artist);
  fireEvent.click(screen.getByRole("button", { name: "Check availability" }));
  await screen.findByText("Audio is ready to play.");
  expect(fetch.mock.calls[4][0]).toBe(`${base}/refresh`);
  expect(fetch.mock.calls[4][1]?.body).toBeUndefined();
  expect(player.queueSnapshot().total).toBe(0);
  expect(play).not.toHaveBeenCalled();
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Play track" }));
  });
  await waitFor(() => expect(play).toHaveBeenCalledTimes(1));
  expect(player.snapshot().current?.id).toBe(savedRecording.id);
  expect(player.snapshot().current?.title).toBe(savedRecording.title);
  expect(document.querySelector("audio")?.src).toContain(
    "/accepted-video-audio/audio",
  );
  expect(fetch.mock.calls.map(([path]) => path)).toEqual([
    "/api/v1/plugins",
    "/api/v1/plugins/video-connection/search",
    `${base}/review`,
    `${base}/attach`,
    `${base}/refresh`,
    base,
  ]);
  for (const [, options] of fetch.mock.calls) {
    expect(options?.headers).toEqual(
      expect.objectContaining({ "x-hito-account": "alice" }),
    );
    expect(options?.signal).toBeInstanceOf(AbortSignal);
  }
  unmount();
  expect(
    fetch.mock.calls.every(([, options]) => options?.signal?.aborted),
  ).toBe(true);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
  expect(errors).not.toHaveBeenCalled();
});

test("a delayed review cannot cross an account and recording change or load the new account's services", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const bobObservation = {
    ...sourceAttachmentPage.detail.selected_observation!,
    id: "75df5bd2-b817-4279-9044-29a2cf6c064c",
    title: "Bob saved title",
    artist: "Bob saved artist",
    provider_recording_id: "spotify/bob",
  };
  const bob: RecordingDetailsPageProps = {
    ...sourceAttachmentPage,
    user: { id: "bob", username: "Bob", admin: false },
    playlists: [],
    detail: {
      recording_id: recordingB,
      title: {
        value: bobObservation.title,
        inherited: bobObservation.title,
        personal: null,
        operation_id: null,
      },
      artist: {
        value: bobObservation.artist,
        inherited: bobObservation.artist,
        personal: null,
        operation_id: null,
      },
      selected_observation: bobObservation,
      observations: { items: [bobObservation], next_before: null },
    },
  };
  const bobSettings = {
    ...attachmentSettings,
    installations: [
      {
        ...attachmentSettings.installations[0],
        id: "bob-video-connection",
        revision: 2,
      },
    ],
  };
  const reviewPath = `/api/v1/recordings/${savedRecording.id}/sources/review`;
  let completeReview!: (response: Response) => void;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    if (path === "/api/v1/plugins") {
      const account = new Headers(options?.headers).get("x-hito-account");
      return new Response(
        JSON.stringify(account === "bob" ? bobSettings : attachmentSettings),
      );
    }
    if (path === "/api/v1/plugins/video-connection/search")
      return new Response(JSON.stringify(attachmentSearch));
    if (path === reviewPath)
      return new Promise<Response>((resolve) => {
        completeReview = resolve;
      });
    throw new Error(
      "An obsolete review must not start another workflow request",
    );
  });
  vi.stubGlobal("fetch", fetch);
  const view = (props: RecordingDetailsPageProps) =>
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: props.user.id,
        children: createElement(RecordingDetailsPage, props),
      }),
    );
  const { rerender, unmount } = render(view(sourceAttachmentPage));
  fireEvent.click(screen.getByRole("button", { name: "Find a source" }));
  await screen.findByRole("combobox", { name: "Music service" });
  fireEvent.change(screen.getByRole("searchbox", { name: "Search music" }), {
    target: { value: "Owned track audio" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Search" }));
  fireEvent.click(await screen.findByRole("button", { name: "Review match" }));
  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(3));
  expect(fetch.mock.calls[2][0]).toBe(reviewPath);
  const reviewSignal = fetch.mock.calls[2][1]?.signal;
  expect(reviewSignal).toBeInstanceOf(AbortSignal);
  expect(reviewSignal?.aborted).toBe(false);
  rerender(view(bob));
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
    "Bob saved title",
  );
  expect(reviewSignal?.aborted).toBe(true);
  expect(fetch).toHaveBeenCalledTimes(3);
  expect(screen.queryByRole("combobox", { name: "Music service" })).toBeNull();
  expect(screen.getByRole("button", { name: "Find a source" })).toBeTruthy();
  await act(async () => {
    completeReview(new Response(JSON.stringify(attachmentReview)));
  });
  expect(screen.queryByText(attachmentReview.candidate.title)).toBeNull();
  expect(screen.queryByText("Search result title")).toBeNull();
  expect(
    screen.queryByRole("region", { name: "Review source match" }),
  ).toBeNull();
  expect(screen.queryByRole("button", { name: "Use this source" })).toBeNull();
  expect(fetch).toHaveBeenCalledTimes(3);
  expect(fetch.mock.calls.some(([path]) => path.endsWith("/attach"))).toBe(
    false,
  );
  expect(play).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Find a source" }));
  const service = await screen.findByRole("combobox", {
    name: "Music service",
  });
  expect((service as HTMLSelectElement).value).toBe("bob-video-connection");
  expect(fetch).toHaveBeenCalledTimes(4);
  expect(fetch.mock.calls[3][0]).toBe("/api/v1/plugins");
  expect(fetch.mock.calls[3][1]?.headers).toEqual(
    expect.objectContaining({ "x-hito-account": "bob" }),
  );
  expect(screen.queryByRole("button", { name: "Use this source" })).toBeNull();
  unmount();
  expect(fetch.mock.calls[3][1]?.signal?.aborted).toBe(true);
});
