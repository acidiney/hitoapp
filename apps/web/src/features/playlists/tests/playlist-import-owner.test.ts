import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistIndexPage from "../pages/PlaylistIndexPage";
import { openImport } from "../../../test/playlistDialogs";
import { NoticeProvider } from "../../../shared/ui/Notices";
import {
  otherPlaylistConnection,
  playlistConnection,
  playlistImportPage,
} from "../fixtures/playlistImports";

import { heldResponse } from "../../../test/async";

vi.mock("@inertiajs/react", () => ({
  router: { visit: vi.fn(), reload: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

const props: Schema<"PlaylistIndexPageProps"> = {
  ...playlistImportPage,
  connections: [...playlistImportPage.connections, otherPlaylistConnection],
  service_names: {
    ...playlistImportPage.service_names,
    [otherPlaylistConnection.service_id]: "Other playlist collection",
  },
};

function renderPage(page: Schema<"PlaylistIndexPageProps">) {
  const view = render(createElement(PlaylistIndexPage, page), {
    wrapper: ({ children }) => createElement(NoticeProvider, null, children),
    reactStrictMode: true,
  });
  openImport();
  return view;
}

function submitTwice(reference: string) {
  const field = screen.getByRole("textbox", {
    name: "Playlist link or identifier",
  });
  fireEvent.change(field, { target: { value: reference } });
  const form = field.closest("form");
  if (!form) throw new Error("Playlist import needs a form");
  act(() => {
    fireEvent.submit(form);
    fireEvent.submit(form);
  });
}

const boundaries = ["account", "removal", "unmount"] as const;
const outcomes = ["success", "failure"] as const;
test.each(
  boundaries.flatMap((boundary) =>
    outcomes.map((outcome) => ({ boundary, outcome })),
  ),
)(
  "$boundary fences an old $outcome without disturbing a current import",
  async ({ boundary, outcome }) => {
    const logging = (["debug", "info", "warn", "error"] as const).map((level) =>
      vi.spyOn(console, level).mockImplementation(() => {}),
    );
    const push = vi.spyOn(history, "pushState");
    const replace = vi.spyOn(history, "replaceState");
    const old = heldResponse();
    const current = heldResponse();
    const fetch = vi
      .fn<(path: string, options: RequestInit) => Promise<Response>>()
      .mockReturnValueOnce(old.response)
      .mockReturnValueOnce(current.response);
    vi.stubGlobal("fetch", fetch);
    let view = renderPage(props);
    const oldReference = `https://provider.invalid/private-${boundary}-${outcome}-old`;
    const currentReference = `https://provider.invalid/private-${boundary}-${outcome}-current`;
    submitTwice(oldReference);
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(fetch.mock.calls[0][0]).toBe(
      `/api/v1/plugins/${playlistConnection.id}/playlists/import`,
    );
    expect(fetch.mock.calls[0][1].headers).toMatchObject({
      "x-hito-account": props.user.id,
    });
    const original: Schema<"ImportProviderPlaylist"> = JSON.parse(
      String(fetch.mock.calls[0][1].body),
    );
    const oldSignal = fetch.mock.calls[0][1].signal;
    expect(oldSignal?.aborted).toBe(false);
    expect(original).toEqual({
      request_id: expect.any(String),
      installation_revision: playlistConnection.revision,
      playlist_reference: oldReference,
    });

    let account = props.user.id;
    let connection = otherPlaylistConnection;
    if (boundary === "account") {
      account = "bob";
      view.rerender(
        createElement(PlaylistIndexPage, {
          ...props,
          user: { id: account, username: "Bob", admin: false },
          connections: [connection],
        }),
      );
    } else if (boundary === "removal") {
      view.rerender(
        createElement(PlaylistIndexPage, {
          ...props,
          connections: [connection],
        }),
      );
    } else {
      connection = playlistConnection;
      view.unmount();
      expect(oldSignal?.aborted).toBe(true);
      view = renderPage(props);
    }
    expect(oldSignal?.aborted).toBe(true);
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(router.visit).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByRole("button", { name: "Retry import" })).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Import a new copy" }),
    ).toBeNull();
    expect(screen.getByRole("button", { name: "Import a copy" })).toBeTruthy();
    if (boundary === "account" || boundary === "unmount")
      expect(
        screen
          .getByRole("textbox", { name: "Playlist link or identifier" })
          .getAttribute("value"),
      ).toBe("");

    submitTwice(currentReference);
    expect(fetch).toHaveBeenCalledTimes(2);
    const replacement: Schema<"ImportProviderPlaylist"> = JSON.parse(
      String(fetch.mock.calls[1][1].body),
    );
    expect(fetch.mock.calls[1][0]).toBe(
      `/api/v1/plugins/${connection.id}/playlists/import`,
    );
    expect(fetch.mock.calls[1][1]).toMatchObject({
      method: "POST",
      credentials: "same-origin",
      headers: { "x-hito-account": account },
    });
    expect(replacement).toEqual({
      request_id: expect.any(String),
      installation_revision: connection.revision,
      playlist_reference: currentReference,
    });
    expect(replacement.request_id).not.toBe(original.request_id);
    expect(fetch.mock.calls[1][1].signal).not.toBe(oldSignal);
    expect(fetch.mock.calls[1][1].signal?.aborted).toBe(false);

    const obsoleteError = `Private obsolete failure for ${oldReference}`;
    await act(async () => {
      if (outcome === "success") {
        const receipt: Schema<"PlaylistImportReceipt"> = {
          request_id: original.request_id,
          playlist_id: "7c209e10-583d-4280-a6da-398d99d6cf7d",
        };
        old.resolve(new Response(JSON.stringify(receipt)));
      } else old.reject(new TypeError(obsoleteError));
    });
    expect(router.visit).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("progressbar")).toBeTruthy();
    expect(fetch).toHaveBeenCalledTimes(2);
    const receipt: Schema<"PlaylistImportReceipt"> = {
      request_id: replacement.request_id,
      playlist_id: "a85a0949-35e6-4c3f-af55-9dc0b718bd06",
    };
    await act(async () =>
      current.resolve(new Response(JSON.stringify(receipt))),
    );
    await waitFor(() =>
      expect(router.visit).toHaveBeenCalledExactlyOnceWith(
        `/playlists/${receipt.playlist_id}`,
      ),
    );
    expect(fetch).toHaveBeenCalledTimes(2);

    const logged = logging.flatMap((sink) => sink.mock.calls);
    for (const [, fields] of logged) {
      expect(Object.getPrototypeOf(fields)).toBe(Object.prototype);
      for (const value of Object.values(fields))
        expect(["string", "number", "boolean"]).toContain(typeof value);
    }
    expect(
      logged
        .filter((call) => call[1]?.event === "playlist_import.started")
        .map((call) => call[1].operation_id),
    ).toEqual([original.request_id, replacement.request_id]);
    expect(
      logged
        .filter((call) => call[1]?.event === "playlist_import.completed")
        .map((call) => call[1].operation_id),
    ).toEqual([replacement.request_id]);
    const observations = JSON.stringify({
      logged,
      urls: fetch.mock.calls.map(([path]) => path),
      navigation: vi.mocked(router.visit).mock.calls,
      history: [...push.mock.calls, ...replace.mock.calls],
      location: location.href,
      storage: [Object.entries(localStorage), Object.entries(sessionStorage)],
    });
    for (const privateValue of [
      oldReference,
      currentReference,
      obsoleteError,
      "playlist_reference",
    ])
      expect(observations).not.toContain(privateValue);
    view.unmount();
  },
);
