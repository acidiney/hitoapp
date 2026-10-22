import { createElement } from "react";
import { render } from "@testing-library/react";
import { vi } from "vitest";
import { NoticeProvider } from "../../shared/ui/Notices";
import type { ResolveSource, Source, SourceResolution } from "../../shared/types";
import type { Player, Track } from "../model";
import { PlaybackProvider, usePlayer } from "../PlaybackProvider";
import PlaybackStatus from "../PlaybackStatus";

export type OwnedPlaybackRequest = {
  path: string;
  method: string;
  account: string | null;
  body: unknown;
  signal: AbortSignal;
};

export function ownedPlaybackHttp(
  reply: (request: OwnedPlaybackRequest) => Response | Promise<Response>,
) {
  const requests: OwnedPlaybackRequest[] = [];
  const unexpected: string[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, options?: RequestInit) => {
      const request: OwnedPlaybackRequest = {
        path: String(input),
        method: options?.method ?? "GET",
        account: new Headers(options?.headers).get("x-hito-account"),
        body:
          options?.body == null ? undefined : JSON.parse(String(options.body)),
        signal: options?.signal as AbortSignal,
      };
      requests.push(request);
      if (
        !(request.signal instanceof AbortSignal) ||
        options?.credentials !== "same-origin"
      ) {
        unexpected.push(request.path);
        throw new Error("Owned playback requires scoped requests");
      }
      try {
        return await reply(request);
      } catch (error) {
        unexpected.push(request.path);
        throw error;
      }
    }),
  );
  return { requests, unexpected };
}

export function renderPlaybackAccount(account: string) {
  const players = new Map<string, Player>();
  function Capture({ account }: { account: string }) {
    players.set(account, usePlayer());
    return null;
  }
  const view = (account: string) =>
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account,
        children: createElement(
          "div",
          null,
          createElement(Capture, { account }),
          createElement(PlaybackStatus),
        ),
      }),
    );
  return { ...render(view(account)), players, view };
}

export const cachedSourcesPath = (track: Track) =>
  `/api/v1/recordings/${track.id}/playback-sources`;
export const sourcesPath = (track: Track) =>
  `/api/v1/recordings/${track.id}/sources`;
export const ownedTrack = (id: string): Track => ({
  id,
  title: "Private title canary",
  artist: "Private artist canary",
  duration_ms: 90000,
  available: false,
});
export const ownedSource = (track: Track, id: string): Source => ({
  id,
  recording_id: track.id,
  provider: "plugin",
  device_id: null,
  ready: true,
  label: "Owned audio service",
  can_prepare: false,
});
export function unavailableResponse(
  request: OwnedPlaybackRequest,
  track: Track,
) {
  const body = request.body as ResolveSource;
  return Response.json({
    request_id: body.request_id,
    resolution_id: "6065e385-46bb-47cc-ac68-2cadf61b6d7c",
    recording_id: track.id,
    outcome: "provider_unavailable",
    sources: [],
  } satisfies SourceResolution);
}
