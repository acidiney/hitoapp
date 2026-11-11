import { createElement, type ReactNode } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { vi } from "vitest";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import RecordingDetailsPage from "../../recordings/pages/RecordingDetailsPage";
import { automaticChoice } from "../../sources/fixtures/sourceChoice";
import { lyricRecordingPage } from "./recordingLyrics";
import { recordingId } from "./lyricIdentityWire";

export type Route =
  | "services"
  | "identities"
  | "candidates"
  | "review"
  | "retain"
  | "read"
  | "remove"
  | "legacy_providers"
  | "legacy_read"
  | "audio";

export type Call = {
  route: Route;
  method: string;
  path: string;
  body: unknown;
  headers: Headers;
  signal: AbortSignal | null;
};

type Reply = Response | Promise<Response>;
export type Handler = (call: Call) => Reply;

const base = `/api/v1/recordings/${recordingId}`;

function classify(method: string, path: string): Route | null {
  const [pathname] = path.split("?");
  const table: [string, string, Route][] = [
    ["GET", `${base}/lyrics/services`, "services"],
    ["GET", `${base}/lyrics/identities`, "identities"],
    ["POST", `${base}/lyrics/candidates`, "candidates"],
    ["POST", `${base}/lyrics/identities/review`, "review"],
    ["POST", `${base}/lyrics/identities`, "retain"],
    ["POST", `${base}/lyrics/identities/read`, "read"],
    ["GET", `${base}/lyrics/providers`, "legacy_providers"],
    ["POST", `${base}/lyrics`, "legacy_read"],
  ];
  for (const [verb, route, name] of table)
    if (method === verb && pathname === route) return name;
  if (method === "DELETE" && pathname.startsWith(`${base}/lyrics/identities/`))
    return "remove";
  if (/\/(sources|download|metadata|stream|audio)\b/.test(pathname))
    return "audio";
  return null;
}

export function json(value: unknown, status = 200): Response {
  return Response.json(value, { status });
}

export function held() {
  let release!: (response: Response) => void;
  const reply = new Promise<Response>((resolve) => {
    release = resolve;
  });
  return { reply, release };
}

export function lyricServer(
  handlers: Partial<Record<Route, Handler>>,
  page = lyricRecordingPage,
) {
  const calls: Call[] = [];
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === `${base}/download`)
      return json({ error: "Not found" }, 404);
    if (method === "GET" && path === `${base}/source-choice`)
      return json(automaticChoice);
    const route = classify(method, path);
    if (!route) throw new Error(`Unexpected request ${method} ${path}`);
    const call: Call = {
      route,
      method,
      path,
      body: options?.body ? JSON.parse(options.body as string) : undefined,
      headers: new Headers(options?.headers),
      signal: options?.signal ?? null,
    };
    calls.push(call);
    const handler = handlers[route];
    if (!handler) throw new Error(`Unhandled lyric route ${route}`);
    return handler(call);
  });
  vi.stubGlobal("fetch", fetch);
  return {
    fetch,
    calls,
    of: (route: Route) => calls.filter((c) => c.route === route),
  };
}

export function renderLyricsPage(
  page = lyricRecordingPage,
  wrap: (children: ReactNode) => ReactNode = (children) => children,
) {
  window.history.replaceState(null, "", `/recordings/${recordingId}`);
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const view = render(
    createElement(
      NoticeProvider,
      null,
      createElement(ArtworkProvider, {
        account: page.user.id,
        children: createElement(PlaybackProvider, {
          account: page.user.id,
          children: wrap(createElement(RecordingDetailsPage, page)),
        }),
      }),
    ),
  );
  return { ...view, play };
}

export function openLyricsTab() {
  fireEvent.click(screen.getByRole("tab", { name: /^Lyrics/ }));
  return within(screen.getByRole("region", { name: "Lyrics" }));
}

export async function openChange(lyrics: ReturnType<typeof openLyricsTab>) {
  fireEvent.click(await lyrics.findByRole("button", { name: "Change" }));
  return within(
    await screen.findByRole("dialog", { name: "Lyrics for this track" }),
  );
}

export async function openFinder(lyrics: ReturnType<typeof openLyricsTab>) {
  const opener = await waitFor(() => {
    const found = lyrics.queryByRole("button", {
      name: /^(Find lyrics|Change)$/,
    });
    if (!found) throw new Error("no way to find lyrics yet");
    return found;
  });
  if (opener.textContent === "Find lyrics") {
    fireEvent.click(opener);
    return;
  }
  fireEvent.click(opener);
  const dialog = within(
    await screen.findByRole("dialog", { name: "Lyrics for this track" }),
  );
  fireEvent.click(dialog.getByRole("button", { name: "Find lyrics" }));
}
