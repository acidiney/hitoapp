// @vitest-environment jsdom
import { createElement } from "react";
import { act, cleanup, fireEvent } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import Artwork from "../components/Artwork";
import type { ArtworkFetch } from "../types";
import { browserPreparationMedia } from "./fixtures/browserPreparationMedia";
import { browserMediaSession } from "./fixtures/browserMediaSession";
import { browserSessionImages } from "./fixtures/browserSessionImages";
import {
  otherRecording,
  playbackRequests,
  recording,
  renderAccount,
} from "./fixtures/browserMediaSessionPlayback";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("held image loads cannot publish after track replacement, account replacement, or queue clearing", async () => {
  const media = browserPreparationMedia();
  const system = browserMediaSession();
  const images = browserSessionImages();
  const requests = playbackRequests([recording, otherRecording]);
  const view = renderAccount();
  try {
    await act(async () => {
      await view.player().playQueue([recording]);
    });
    images.size(images.created[0]);
    const firstLoad = images.holdLoad(images.created[0]);
    await act(async () => {
      await view.player().playQueue([otherRecording]);
    });
    expect(images.created).toHaveLength(2);
    expect(images.created[0].getAttribute("src")).toBeNull();
    act(firstLoad);
    expect(system.session.metadata?.title).toBe(otherRecording.title);
    expect(system.session.metadata?.artwork).toEqual([]);

    images.size(images.created[1]);
    const secondLoad = images.holdLoad(images.created[1]);
    view.rerender(view.accountView("second-owned-account"));
    expect(system.session.metadata).toBeNull();
    await act(async () => {
      await view.player().playQueue([recording]);
    });
    expect(images.created).toHaveLength(3);
    act(() => {
      firstLoad();
      secondLoad();
    });
    expect(system.session.metadata?.title).toBe(recording.title);
    expect(system.session.metadata?.artwork).toEqual([]);
    images.size(images.created[2]);
    const thirdLoad = images.holdLoad(images.created[2]);
    act(() => view.player().clear());
    act(thirdLoad);
    expect(system.session.metadata).toBeNull();
    expect(
      images.created.every((image) => image.getAttribute("src") === null),
    ).toBe(true);
    expect(requests).toHaveLength(3);
    expect(
      requests.every(
        (request) =>
          request.method === "GET" &&
          request.path.endsWith("/playback-sources"),
      ),
    ).toBe(true);
  } finally {
    view.unmount();
    system.close();
    media.close();
  }
});

test.each(["error", "timeout"] as const)(
  "an artwork %s ends its bounded probe without provider retrieval or changing playback",
  async (outcome) => {
    vi.useFakeTimers();
    const media = browserPreparationMedia();
    const system = browserMediaSession();
    const images = browserSessionImages();
    const requests = playbackRequests();
    const view = renderAccount();
    try {
      await act(async () => {
        await view.player().playQueue([recording]);
      });
      const probe = images.created[0];
      images.size(probe);
      const lateLoad = images.holdLoad(probe);
      if (outcome === "error")
        act(() => probe.dispatchEvent(new Event("error")));
      else
        await act(async () => {
          await vi.advanceTimersByTimeAsync(10_000);
        });
      expect(probe.getAttribute("src")).toBeNull();
      expect(system.session.metadata?.artwork).toEqual([]);
      act(lateLoad);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(60_000);
      });
      expect(images.created).toHaveLength(1);
      expect(system.session.metadata?.artwork).toEqual([]);
      expect(view.player().snapshot().playing).toBe(true);
      expect(requests).toHaveLength(1);
    } finally {
      view.unmount();
      system.close();
      media.close();
    }
  },
);

test("a current cover retries only when the existing authorized artwork workflow publishes a ready revision", async () => {
  const media = browserPreparationMedia();
  const system = browserMediaSession();
  const images = browserSessionImages();
  const path = `/api/v1/recordings/${recording.id}/artwork`;
  const requests = playbackRequests([recording], (requested, options) => {
    if (requested === `${path}/availability` && options?.method === "GET")
      return Response.json({ can_fetch: true });
    if (requested === `${path}/fetch` && options?.method === "POST")
      return Response.json({
        id: "3ef96135-2f18-497d-b62e-cba8072cbab8",
        recording_id: recording.id,
        status: "ready",
        attempts: 1,
        error: null,
        created_at: 1,
        updated_at: 1,
      } satisfies ArtworkFetch);
    return undefined;
  });
  const view = renderAccount(false, {
    artwork: true,
    children: createElement(Artwork, { recording: recording.id }),
  });
  try {
    await act(async () => {
      await view.player().playQueue([recording]);
    });
    act(() => images.created[0].dispatchEvent(new Event("error")));
    expect(system.session.metadata?.artwork).toEqual([]);
    expect(requests).toHaveLength(1);
    const cover = view.container.querySelector("img");
    if (!cover)
      throw new Error("The current artwork component must be mounted");
    await act(async () => {
      fireEvent.error(cover);
    });
    await vi.waitFor(() => expect(images.created).toHaveLength(2));
    act(() => images.load(images.created[1], 512, 512));
    expect(system.session.metadata?.artwork).toEqual([
      {
        src: new URL(path, location.href).href,
        sizes: "512x512",
      },
    ]);
    expect(
      requests.map((request) => `${request.method} ${request.path}`),
    ).toEqual([
      `GET /api/v1/recordings/${recording.id}/playback-sources`,
      `GET ${path}/availability`,
      `POST ${path}/fetch`,
    ]);
    expect(
      requests.every((request) => request.account === "owned-system-account"),
    ).toBe(true);
  } finally {
    view.unmount();
    system.close();
    media.close();
  }
});
