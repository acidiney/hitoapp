import { createElement, StrictMode, type ReactNode } from "react";
import { render } from "@testing-library/react";
import { vi } from "vitest";
import { NoticeProvider } from "../../shared/ui/Notices";
import type { Source, SyncBatch } from "../../shared/types";
import type { Player, Track } from "../model";
import { PlaybackProvider, usePlayer } from "../PlaybackProvider";
import { ArtworkProvider } from "../../features/artwork/components/ArtworkProvider";

export const recording: Track = {
  id: "b36566e6-211b-4d2e-8816-a6a4f175a97f",
  title: "Private owned title",
  artist: "Private owned artist",
  duration_ms: 90000,
  available: true,
};
export const otherRecording: Track = {
  ...recording,
  id: "9e725279-2136-4429-a07f-e3b1e5be86a3",
  title: "Second private owned title",
};
const source: Source = {
  id: "49858203-ce08-478f-95fe-482f34e376fb",
  recording_id: recording.id,
  provider: "local",
  device_id: null,
  ready: true,
  label: "Owned audio",
  can_prepare: false,
};

export function playbackRequests(
  tracks: readonly Track[] = [recording],
  reply?: (
    path: string,
    options?: RequestInit,
  ) => Response | Promise<Response> | undefined,
) {
  const requests: { path: string; method: string; account: string | null }[] =
    [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, options?: RequestInit) => {
      const path = String(input);
      const method = options?.method ?? "GET";
      requests.push({
        path,
        method,
        account: new Headers(options?.headers).get("x-hito-account"),
      });
      const track = tracks.find(
        (track) => path === `/api/v1/recordings/${track.id}/playback-sources`,
      );
      if (track && method === "GET")
        return Response.json([
          {
            ...source,
            recording_id: track.id,
            id: track.id === recording.id ? source.id : track.id,
          },
        ]);
      if (path === "/api/v1/sync" && method === "POST") {
        const batch = JSON.parse(String(options?.body)) as SyncBatch;
        return Response.json({
          acknowledged: batch.operations.map((operation) => operation.id),
        });
      }
      const response = reply?.(path, options);
      if (response) return response;
      throw new Error(
        "System metadata must not resolve sources or download media",
      );
    }),
  );
  return requests;
}

export function renderAccount(
  strict = false,
  options: { artwork?: boolean; children?: ReactNode } = {},
) {
  let player!: Player;
  let renders = 0;
  function Capture() {
    player = usePlayer();
    renders++;
    return options.children ?? null;
  }
  const accountView = (account: string) => {
    const playback = createElement(PlaybackProvider, {
      account,
      children: createElement(Capture),
    });
    const content = createElement(
      NoticeProvider,
      null,
      options.artwork
        ? createElement(ArtworkProvider, { account, children: playback })
        : playback,
    );
    return strict ? createElement(StrictMode, null, content) : content;
  };
  const view = render(accountView("owned-system-account"));
  return { ...view, player: () => player, accountView, renders: () => renders };
}
