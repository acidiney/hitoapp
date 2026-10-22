import { createElement } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { NoticeProvider } from "../shared/ui/Notices";
import TrackToolbar from "../shared/ui/TrackToolbar";
import type { Player, Track } from "./model";
import { PlaybackProvider, usePlayer } from "./PlaybackProvider";

const recordings = {
  alice: "1182af62-6979-494d-8bf0-98e9db393e20",
  bob: "b16fbf0d-ac02-427d-b8dc-498d04ccf74a",
};
const source = (recording: string) => ({
  id:
    recording === recordings.alice
      ? "0557f5e4-8373-40bb-979e-40095e18cde2"
      : "08a3097c-682e-4608-9f06-1e8739ee9705",
  recording_id: recording,
  provider: "plugin",
  device_id: null,
  ready: true,
  label: "Owned audio service",
  can_prepare: false,
});
const response = (body: unknown) =>
  new Response(JSON.stringify(body), {
    headers: { "content-type": "application/json" },
  });

test("ordinary Play resolves stale availability within its account owner and ignores a cancelled account response", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi
    .spyOn(HTMLMediaElement.prototype, "play")
    .mockImplementation(async function (this: HTMLMediaElement) {
      this.dispatchEvent(new Event("play"));
    });
  const requests: {
    path: string;
    method: string;
    account: string;
    body: unknown;
    signal: AbortSignal;
  }[] = [];
  let releaseAlice!: (value: Response) => void;
  const aliceResponse = new Promise<Response>((resolve) => {
    releaseAlice = resolve;
  });
  let aliceRequest = "";
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, options?: RequestInit) => {
      const path = String(input);
      const account = (options?.headers as Record<string, string>)[
        "x-hito-account"
      ];
      const recording = recordings[account as keyof typeof recordings];
      const body =
        options?.body == null ? undefined : JSON.parse(String(options.body));
      if (!recording || !(options?.signal instanceof AbortSignal))
        throw new Error("Unexpected owned account request");
      requests.push({
        path,
        method: options.method ?? "GET",
        account,
        body,
        signal: options.signal,
      });
      if (
        path === `/api/v1/recordings/${recording}/playback-sources` &&
        options.method === "GET"
      )
        return response([]);
      if (
        path === `/api/v1/recordings/${recording}/sources/refresh` &&
        options.method === "POST"
      ) {
        expect(body).toEqual({ purpose: "playback" });
        return response([]);
      }
      if (
        path !== `/api/v1/recordings/${recording}/sources/resolve` ||
        options.method !== "POST"
      )
        throw new Error("Unexpected owned playback workflow request");
      expect(Object.keys(body)).toEqual(["request_id"]);
      expect(body.request_id).toMatch(
        /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i,
      );
      if (account === "alice") {
        aliceRequest = body.request_id;
        return aliceResponse;
      }
      return response({
        request_id: body.request_id,
        resolution_id: "b98a8058-ef2e-4fa6-a054-11e27b666beb",
        recording_id: recording,
        outcome: "ready",
        sources: [source(recording)],
      });
    }),
  );
  const players = new Map<string, Player>();
  function Capture({ account }: { account: string }) {
    players.set(account, usePlayer());
    return null;
  }
  const view = (account: keyof typeof recordings) => {
    const track: Track = {
      id: recordings[account],
      title: "Personal display title",
      artist: "Personal display artist",
      duration_ms: 12000,
      available: false,
    };
    return createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account,
        children: createElement(
          "div",
          null,
          createElement(Capture, { account }),
          createElement(TrackToolbar, {
            tracks: account === "alice" ? [track, track] : [track],
            label: "playlist",
            description: "Owned imported playlist",
          }),
        ),
      }),
    );
  };
  const { rerender } = render(view("alice"));
  expect(requests).toHaveLength(0);
  const button = screen.getByRole("button", {
    name: "Play playlist",
  }) as HTMLButtonElement;
  expect(button.disabled).toBe(false);
  fireEvent.click(button);
  await vi.waitFor(() => expect(aliceRequest).not.toBe(""));
  expect(requests.map((request) => request.method)).toEqual([
    "GET",
    "POST",
    "POST",
  ]);
  const old = players.get("alice")!;
  const entries = await old.queuePage(0, 10);
  expect(entries.entries.map((entry) => entry.recording.id)).toEqual([
    recordings.alice,
    recordings.alice,
  ]);
  expect(new Set(entries.entries.map((entry) => entry.id)).size).toBe(2);
  const abandoned = requests[2];
  rerender(view("bob"));
  expect(abandoned.signal.aborted).toBe(true);
  expect(old.snapshot().current).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Play playlist" }));
  await vi.waitFor(() =>
    expect(players.get("bob")?.snapshot().playing).toBe(true),
  );
  expect(requests.map((request) => request.account)).toEqual([
    "alice",
    "alice",
    "alice",
    "bob",
    "bob",
    "bob",
  ]);
  expect(document.querySelector("audio")?.src).toContain(
    `/sources/${source(recordings.bob).id}/audio`,
  );
  await act(async () => {
    releaseAlice(
      response({
        request_id: aliceRequest,
        resolution_id: "3be9c605-dd17-4caf-b46e-71a442c65180",
        recording_id: recordings.alice,
        outcome: "ready",
        sources: [source(recordings.alice)],
      }),
    );
    await aliceResponse;
  });
  expect(play).toHaveBeenCalledTimes(1);
  expect(document.querySelectorAll("audio")).toHaveLength(1);
  expect(document.querySelector("audio")?.src).toContain(
    `/sources/${source(recordings.bob).id}/audio`,
  );
  expect(players.get("bob")?.snapshot().current?.id).toBe(recordings.bob);
  expect(players.get("bob")?.snapshot().error).toBe("");
  expect((await players.get("bob")!.queuePage(0, 10)).entries).toHaveLength(1);
});
