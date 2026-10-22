import { createElement, type AnchorHTMLAttributes } from "react";
import { act, fireEvent, render } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import Shell from "./Shell";
import Artwork from "./components/Artwork";
import type { ArtworkFetch } from "./types";

vi.mock("@inertiajs/react", () => ({
  usePage: () => ({
    component: "CatalogPage",
    url: "/library",
    props: {
      user: { id: "alice", username: "Alice", admin: false },
      errors: {},
      native_playback: false,
      libraries: [],
      playlists: [],
    },
  }),
  router: { get: vi.fn(), clearHistory: vi.fn() },
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

test("returning to a recording resumes artwork without accepting its abandoned response", async () => {
  vi.useFakeTimers();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const recording = "b3845d83-bcf8-42b2-8ef7-321f6f8708d2";
  const artwork = `/api/v1/recordings/${recording}/artwork`;
  const availability = `${artwork}/availability`;
  const enqueue = `${artwork}/fetch`;
  let releaseOldAvailability!: (response: Response) => void;
  const oldAvailability = new Promise<Response>((resolve) => {
    releaseOldAvailability = resolve;
  });
  let availabilityReads = 0;
  let pending = 0;
  let peakPending = 0;
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    let response: Promise<Response>;
    if (path === availability && options?.method === "GET") {
      availabilityReads++;
      response =
        availabilityReads === 1
          ? oldAvailability
          : Promise.resolve(Response.json({ can_fetch: true }));
    } else if (path === enqueue && options?.method === "POST") {
      response = Promise.resolve(
        Response.json(
          {
            id: "3d185994-6a12-43fc-962b-911e7a77381e",
            recording_id: recording,
            status: "ready",
            attempts: 1,
            error: null,
            created_at: 1,
            updated_at: 2,
          } satisfies ArtworkFetch,
          { status: 202 },
        ),
      );
    } else {
      throw new Error(
        "Unexpected request outside the artwork navigation workflow",
      );
    }
    pending++;
    peakPending = Math.max(peakPending, pending);
    return response.finally(() => {
      pending--;
    });
  });
  vi.stubGlobal("fetch", fetch);
  const view = (showRecording: boolean) =>
    createElement(Shell, {
      children: showRecording
        ? createElement(Artwork, { recording })
        : createElement("p", null, "Another library page"),
    });
  const { container, rerender } = render(view(true));
  const cover = () => container.querySelector(`main img[src="${artwork}"]`);
  await act(async () => {
    fireEvent.error(cover()!);
  });
  expect(availabilityReads).toBe(1);
  const oldSignal = fetch.mock.calls[0][1]?.signal;
  expect(oldSignal?.aborted).toBe(false);
  expect(pending).toBe(1);

  rerender(view(false));
  expect(oldSignal?.aborted).toBe(true);
  expect(cover()).toBeNull();
  expect(pending).toBe(1);
  rerender(view(true));
  expect(cover()).toBeTruthy();
  await act(async () => {
    fireEvent.error(cover()!);
  });
  expect(availabilityReads).toBe(2);
  expect(cover()?.getAttribute("src")).toBe(artwork);
  expect(peakPending).toBe(2);

  await act(async () => {
    releaseOldAvailability(Response.json({ can_fetch: true }));
  });
  expect(fetch.mock.calls.map(([path]) => path)).toEqual([
    availability,
    availability,
    enqueue,
  ]);
  expect(cover()?.getAttribute("src")).toBe(artwork);
  expect(pending).toBe(0);
  expect(peakPending).toBe(2);
  expect(vi.getTimerCount()).toBe(0);
});
