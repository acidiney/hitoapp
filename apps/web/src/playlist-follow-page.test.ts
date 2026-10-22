import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistIndexPage from "./pages/PlaylistIndexPage";
import { NoticeProvider } from "./components/Notices";
import {
  playlistConnection as connection,
  playlistImportPage as props,
} from "./fixtures/playlistImports";
import type { FollowProviderPlaylist, PlaylistFollowReceipt } from "./types";

vi.mock("@inertiajs/react", () => ({
  router: { visit: vi.fn(), reload: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

test("Playlists follows a provider playlist daily and navigates only after its receipt", async () => {
  let resolve!: (response: Response) => void;
  const response = new Promise<Response>((done) => {
    resolve = done;
  });
  const fetch = vi
    .fn<(path: string, options: RequestInit) => Promise<Response>>()
    .mockReturnValue(response);
  vi.stubGlobal("fetch", fetch);
  render(createElement(PlaylistIndexPage, props), {
    wrapper: ({ children }) => createElement(NoticeProvider, null, children),
  });
  const form = within(
    screen.getByRole("region", { name: "Import a playlist" }),
  );
  expect(form.getByRole("button", { name: "Import a copy" })).toBeTruthy();
  fireEvent.change(form.getByRole("combobox", { name: "Playlist service" }), {
    target: { value: connection.id },
  });
  fireEvent.click(form.getByRole("radio", { name: "Follow playlist" }));
  const interval = form.getByRole("combobox", { name: "Refresh interval" });
  expect(
    form.getByRole("option", { name: "Every hour", selected: true }),
  ).toBeTruthy();
  const daily = form.getByRole("option", {
    name: "Every day",
  }) as HTMLOptionElement;
  fireEvent.change(interval, { target: { value: daily.value } });
  expect(
    form.getByRole("option", { name: "Every day", selected: true }),
  ).toBeTruthy();
  const reference =
    "https://provider.invalid/playlists/private-follow-reference";
  const field = form.getByRole("textbox", {
    name: "Playlist link or identifier",
  });
  fireEvent.change(field, { target: { value: `  ${reference}  ` } });
  expect(fetch).not.toHaveBeenCalled();
  const submit = form.getByRole("button", { name: "Follow playlist" });
  const element = field.closest("form");
  if (!element) throw new Error("Following a playlist needs a form");
  act(() => {
    fireEvent.click(submit);
    fireEvent.submit(element);
  });
  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(1));
  expect(fetch.mock.calls[0][0]).toBe(
    `/api/v1/plugins/${connection.id}/playlists/follow`,
  );
  expect(fetch.mock.calls[0][1]).toMatchObject({
    method: "POST",
    credentials: "same-origin",
    headers: { "x-hito-account": props.user.id },
    signal: expect.any(AbortSignal),
  });
  const request = JSON.parse(String(fetch.mock.calls[0][1].body));
  expect(request).toEqual({
    request_id: expect.stringMatching(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    ),
    installation_revision: connection.revision,
    playlist_reference: reference,
    interval: "daily",
  });
  expect((submit as HTMLButtonElement).disabled).toBe(true);
  expect(router.visit).not.toHaveBeenCalled();
  fireEvent.submit(element);
  expect(fetch).toHaveBeenCalledTimes(1);
  const receipt = {
    request_id: request.request_id,
    playlist_id: "a54eff80-553a-4c96-b360-88ce72794220",
  };
  await act(async () => resolve(new Response(JSON.stringify(receipt))));
  await waitFor(() =>
    expect(router.visit).toHaveBeenCalledExactlyOnceWith(
      `/playlists/${receipt.playlist_id}`,
    ),
  );
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("button", { name: "Create playlist" })).toBeTruthy();
});

test("a lost daily Follow retries its original operation after the draft and connection revision change", async () => {
  let resolveRetry!: (response: Response) => void;
  const retryResponse = new Promise<Response>((done) => {
    resolveRetry = done;
  });
  const fetch = vi
    .fn<(path: string, options: RequestInit) => Promise<Response>>()
    .mockRejectedValueOnce(new TypeError("The follow response was lost."))
    .mockReturnValueOnce(retryResponse);
  vi.stubGlobal("fetch", fetch);
  const view = render(createElement(PlaylistIndexPage, props), {
    wrapper: ({ children }) => createElement(NoticeProvider, null, children),
  });
  const form = within(
    screen.getByRole("region", { name: "Import a playlist" }),
  );
  fireEvent.click(form.getByRole("radio", { name: "Follow playlist" }));
  fireEvent.change(form.getByRole("combobox", { name: "Refresh interval" }), {
    target: { value: "daily" },
  });
  const originalReference =
    "https://provider.invalid/playlists/private-original-follow";
  const draftReference =
    "https://provider.invalid/playlists/private-unsent-copy";
  const field = form.getByRole("textbox", {
    name: "Playlist link or identifier",
  });
  fireEvent.change(field, { target: { value: originalReference } });
  fireEvent.click(form.getByRole("button", { name: "Follow playlist" }));
  await form.findByRole("alert");
  expect(fetch).toHaveBeenCalledTimes(1);
  const originalBody = String(fetch.mock.calls[0][1].body);
  const original: FollowProviderPlaylist = JSON.parse(originalBody);
  expect(original).toEqual({
    request_id: expect.stringMatching(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    ),
    installation_revision: connection.revision,
    playlist_reference: originalReference,
    interval: "daily",
  });
  const oldSignal = fetch.mock.calls[0][1].signal;
  expect(oldSignal?.aborted).toBe(false);
  fireEvent.change(form.getByRole("combobox", { name: "Refresh interval" }), {
    target: { value: "hourly" },
  });
  expect(
    form.getByRole("option", { name: "Every hour", selected: true }),
  ).toBeTruthy();
  fireEvent.click(form.getByRole("radio", { name: "Import a copy" }));
  fireEvent.change(field, { target: { value: draftReference } });
  view.rerender(
    createElement(PlaylistIndexPage, {
      ...props,
      connections: props.connections.map((item) =>
        item.id === connection.id
          ? { ...item, revision: connection.revision + 1 }
          : item,
      ),
    }),
  );
  expect(oldSignal?.aborted).toBe(true);
  expect(form.queryByRole("alert")).toBeNull();
  expect(form.queryByRole("combobox", { name: "Refresh interval" })).toBeNull();
  expect(
    form.getByRole("radio", { name: "Import a copy", checked: true }),
  ).toBeTruthy();
  expect((field as HTMLInputElement).value).toBe(draftReference);
  expect(form.getByRole("button", { name: "Import a copy" })).toBeTruthy();
  expect(form.queryByRole("button", { name: "Retry import" })).toBeNull();
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(router.visit).not.toHaveBeenCalled();
  fireEvent.click(form.getByRole("button", { name: "Retry follow" }));
  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
  expect(fetch.mock.calls.map(([path]) => path)).toEqual([
    `/api/v1/plugins/${connection.id}/playlists/follow`,
    `/api/v1/plugins/${connection.id}/playlists/follow`,
  ]);
  expect(fetch.mock.calls[1][1]).toMatchObject({
    method: "POST",
    credentials: "same-origin",
    headers: { "x-hito-account": props.user.id },
    body: originalBody,
    signal: expect.any(AbortSignal),
  });
  expect(fetch.mock.calls[1][1].signal).not.toBe(oldSignal);
  expect(fetch.mock.calls[1][1].signal?.aborted).toBe(false);
  expect(
    (form.getByRole("button", { name: "Following…" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  expect(form.queryByRole("button", { name: "Importing…" })).toBeNull();
  expect(router.visit).not.toHaveBeenCalled();
  const receipt = {
    request_id: original.request_id,
    playlist_id: "04c1711d-26ac-49b5-97c3-243910d42570",
  } satisfies PlaylistFollowReceipt;
  await act(async () => resolveRetry(new Response(JSON.stringify(receipt))));
  await waitFor(() =>
    expect(router.visit).toHaveBeenCalledExactlyOnceWith(
      `/playlists/${receipt.playlist_id}`,
    ),
  );
  expect(fetch).toHaveBeenCalledTimes(2);
  expect((field as HTMLInputElement).value).toBe(draftReference);
});
