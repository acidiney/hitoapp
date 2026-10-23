import { createElement } from "react";
import { act } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import Artwork from "../../artwork/components/Artwork";
import type { ArtworkFetch } from "../../../shared/types";
import { browserPreparationMedia } from "../fixtures/browserPreparationMedia";
import { browserMediaSession } from "../fixtures/browserMediaSession";
import { browserSessionImages } from "../fixtures/browserSessionImages";
import {
  otherRecording,
  playbackRequests,
  recording,
  renderAccount,
} from "../fixtures/browserMediaSessionPlayback";

const releaseId = "0d9f3a3c-6a4e-4d6e-9a43-0f0c2c0bd0aa";
const recordingPath = (id: string) => `/api/v1/recordings/${id}/artwork`;
const releasePath = `/api/v1/releases/${releaseId}/artwork`;

function job(id: string): ArtworkFetch {
  return {
    id: "3ef96135-2f18-497d-b62e-cba8072cbab8",
    recording_id: id,
    status: "ready",
    attempts: 1,
    error: null,
    created_at: 1,
    updated_at: 1,
  };
}

function workflow(options: { canFetch?: boolean; hold?: Promise<void> } = {}) {
  const { canFetch = true, hold } = options;
  return (path: string, init?: RequestInit) => {
    const method = init?.method ?? "GET";
    const match = /^\/api\/v1\/recordings\/([^/]+)\/artwork\/(availability|fetch)$/.exec(
      path,
    );
    if (!match) {
      if (path.includes("/releases/"))
        return Response.json({ can_fetch: false });
      return undefined;
    }
    if (match[2] === "availability" && method === "GET")
      return Response.json({ can_fetch: canFetch });
    if (match[2] === "fetch" && method === "POST")
      return (async () => {
        await hold;
        return Response.json(job(match[1]));
      })();
    return undefined;
  };
}

function artworkRequests(requests: { path: string; method: string }[]) {
  return requests
    .filter((request) => request.path.includes("/artwork"))
    .map((request) => `${request.method} ${request.path}`);
}

async function setup(
  tracks = [recording, otherRecording],
  reply = workflow(),
) {
  const media = browserPreparationMedia();
  const system = browserMediaSession();
  const images = browserSessionImages();
  const requests = playbackRequests(tracks, reply);
  const view = renderAccount(false, { artwork: true });
  return {
    media,
    system,
    images,
    requests,
    view,
    close() {
      view.unmount();
      system.close();
      media.close();
    },
  };
}

test("a missing current cover requests the shared retrieval once and publishes the ready image", async () => {
  const test = await setup();
  try {
    await act(async () => {
      await test.view.player().playQueue([recording]);
    });
    act(() => test.images.created[0].dispatchEvent(new Event("error")));
    await vi.waitFor(() => expect(test.images.created).toHaveLength(2));
    expect(test.system.session.metadata?.artwork).toEqual([]);
    act(() => test.images.load(test.images.created[1], 512, 512));
    expect(test.system.session.metadata?.artwork).toEqual([
      {
        src: new URL(recordingPath(recording.id), location.href).href,
        sizes: "512x512",
      },
    ]);
    expect(artworkRequests(test.requests)).toEqual([
      `GET ${recordingPath(recording.id)}/availability`,
      `POST ${recordingPath(recording.id)}/fetch`,
    ]);
    expect(
      test.requests.every(
        (request) => request.account === "owned-system-account",
      ),
    ).toBe(true);
  } finally {
    test.close();
  }
});

test("a mounted artwork component and the media session share one fetch", async () => {
  const media = browserPreparationMedia();
  const system = browserMediaSession();
  const images = browserSessionImages();
  const requests = playbackRequests([recording], workflow());
  const view = renderAccount(false, {
    artwork: true,
    children: createElement(Artwork, { recording: recording.id }),
  });
  try {
    await act(async () => {
      await view.player().playQueue([recording]);
    });
    act(() => images.created[0].dispatchEvent(new Event("error")));
    await act(async () => {
      view.container
        .querySelector("img")
        ?.dispatchEvent(new Event("error"));
    });
    await vi.waitFor(() => expect(images.created).toHaveLength(2));
    expect(
      artworkRequests(requests).filter((request) => request.startsWith("POST")),
    ).toHaveLength(1);
  } finally {
    view.unmount();
    system.close();
    media.close();
  }
});

test("a track change during retrieval discards the earlier cover", async () => {
  let release!: () => void;
  const hold = new Promise<void>((resolve) => (release = resolve));
  const test = await setup([recording, otherRecording], workflow({ hold }));
  try {
    await act(async () => {
      await test.view.player().playQueue([recording]);
    });
    act(() => test.images.created[0].dispatchEvent(new Event("error")));
    await vi.waitFor(() =>
      expect(artworkRequests(test.requests)).toContain(
        `POST ${recordingPath(recording.id)}/fetch`,
      ),
    );
    await act(async () => {
      await test.view.player().playQueue([otherRecording]);
    });
    const probes = test.images.created.length;
    await act(async () => {
      release();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(test.images.created).toHaveLength(probes);
    expect(test.system.session.metadata?.title).toBe(otherRecording.title);
    expect(test.system.session.metadata?.artwork).toEqual([]);
    expect(
      test.images.created
        .slice(1)
        .every((image) => !image.src.includes(recording.id)),
    ).toBe(true);
  } finally {
    test.close();
  }
});

test("account replacement during retrieval clears metadata and ignores the late cover", async () => {
  let release!: () => void;
  const hold = new Promise<void>((resolve) => (release = resolve));
  const test = await setup([recording], workflow({ hold }));
  try {
    await act(async () => {
      await test.view.player().playQueue([recording]);
    });
    act(() => test.images.created[0].dispatchEvent(new Event("error")));
    await vi.waitFor(() =>
      expect(artworkRequests(test.requests)).toHaveLength(2),
    );
    test.view.rerender(test.view.accountView("second-owned-account"));
    expect(test.system.session.metadata).toBeNull();
    const probes = test.images.created.length;
    await act(async () => {
      release();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(test.images.created).toHaveLength(probes);
    expect(test.system.session.metadata).toBeNull();
  } finally {
    test.close();
  }
});

test("an unavailable cover is checked once and never loops", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const test = await setup(
    [recording, otherRecording],
    workflow({ canFetch: false }),
  );
  try {
    await act(async () => {
      await test.view.player().playQueue([recording]);
    });
    act(() => test.images.created[0].dispatchEvent(new Event("error")));
    await vi.waitFor(() =>
      expect(artworkRequests(test.requests)).toHaveLength(1),
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(130_000);
    });
    await act(async () => {
      await test.view.player().playQueue([otherRecording]);
      await test.view.player().playQueue([recording]);
    });
    const last = test.images.created.at(-1);
    if (!last) throw new Error("A probe must exist");
    act(() => last.dispatchEvent(new Event("error")));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });
    expect(
      artworkRequests(test.requests).filter(
        (request) => request.includes(recording.id),
      ),
    ).toEqual([`GET ${recordingPath(recording.id)}/availability`]);
    expect(test.system.session.metadata?.artwork).toEqual([]);
  } finally {
    test.close();
    vi.useRealTimers();
  }
});

test("a track with a release tries the release cover before the recording cover", async () => {
  const withRelease = { ...recording, release_id: releaseId };
  const test = await setup([withRelease], workflow());
  try {
    await act(async () => {
      await test.view.player().playQueue([withRelease]);
    });
    expect(new URL(test.images.created[0].src).pathname).toBe(releasePath);
    act(() => test.images.created[0].dispatchEvent(new Event("error")));
    expect(new URL(test.images.created[1].src).pathname).toBe(
      recordingPath(recording.id),
    );
    act(() => test.images.load(test.images.created[1], 300, 300));
    expect(test.system.session.metadata?.artwork).toEqual([
      {
        src: new URL(recordingPath(recording.id), location.href).href,
        sizes: "300x300",
      },
    ]);
    await vi.waitFor(() =>
      expect(
        test.requests.some(
          (request) =>
            request.path === `${releasePath}/availability` &&
            request.method === "GET",
        ),
      ).toBe(true),
    );
  } finally {
    test.close();
  }
});
