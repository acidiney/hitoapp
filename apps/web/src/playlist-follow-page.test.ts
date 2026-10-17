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
import { router } from "@inertiajs/react";
import PlaylistIndexPage from "./pages/PlaylistIndexPage";
import { NoticeProvider } from "./components/Notices";
import {
  playlistConnection as connection,
  playlistImportPage as props,
} from "./fixtures/playlistImports";

vi.mock("@inertiajs/react", () => ({
  router: { visit: vi.fn(), reload: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

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
