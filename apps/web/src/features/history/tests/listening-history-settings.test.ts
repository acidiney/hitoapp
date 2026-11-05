import { createElement } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { isOn } from "../../../test/switch";
import ListeningHistorySettings from "../components/ListeningHistorySettings";

function stubFetch() {
  const fetch = vi.fn<typeof globalThis.fetch>();
  vi.stubGlobal("fetch", fetch);
  return fetch;
}

function calls(fetch: ReturnType<typeof stubFetch>) {
  return fetch.mock.calls.map(([url, init]) => [
    String(url),
    init?.method ?? "GET",
    init?.body ?? null,
  ]);
}

const saveSwitch = () =>
  screen.findByRole("switch", { name: "Save listening history" });

test("the switch shows the saved preference and saves a change at once", async () => {
  const fetch = stubFetch();
  fetch
    .mockResolvedValueOnce(Response.json({ enabled: true }))
    .mockResolvedValueOnce(Response.json({ enabled: false }));
  render(createElement(ListeningHistorySettings, { account: "alice" }));
  const toggle = await saveSwitch();
  expect(isOn(toggle)).toBe(true);
  fireEvent.click(toggle);
  await waitFor(() => expect(isOn(toggle)).toBe(false));
  expect(calls(fetch)).toEqual([
    ["/api/v1/history/preferences", "GET", null],
    ["/api/v1/history/preferences", "PUT", JSON.stringify({ enabled: false })],
  ]);
  expect(screen.getByText(/Listening history is paused/)).toBeTruthy();
});

test("a failed save keeps the accepted preference and says why", async () => {
  const fetch = stubFetch();
  fetch
    .mockResolvedValueOnce(Response.json({ enabled: true }))
    .mockResolvedValueOnce(
      Response.json(
        { error: "The server could not save this" },
        { status: 500 },
      ),
    );
  render(createElement(ListeningHistorySettings, { account: "alice" }));
  const toggle = await saveSwitch();
  fireEvent.click(toggle);
  await screen.findByRole("alert");
  expect(isOn(toggle)).toBe(true);
});

test("delete needs a confirmation, cancelling sends nothing and confirming deletes once", async () => {
  const fetch = stubFetch();
  fetch
    .mockResolvedValueOnce(Response.json({ enabled: true }))
    .mockResolvedValueOnce(Response.json({ removed: 3 }));
  render(createElement(ListeningHistorySettings, { account: "alice" }));
  await saveSwitch();
  fireEvent.click(
    screen.getByRole("button", { name: "Delete listening history" }),
  );
  const dialog = screen.getByRole("dialog", {
    name: "Delete listening history?",
  });
  expect(dialog.textContent).toContain("Likes, playlists, resume positions");
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(calls(fetch)).toHaveLength(1);

  fireEvent.click(
    screen.getByRole("button", { name: "Delete listening history" }),
  );
  fireEvent.click(
    within(screen.getByRole("dialog")).getByRole("button", {
      name: "Delete history",
    }),
  );
  await screen.findByText(/3 entries removed/);
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(calls(fetch)).toEqual([
    ["/api/v1/history/preferences", "GET", null],
    ["/api/v1/history", "DELETE", null],
  ]);
});

test("a failed delete keeps the dialog result out of the page and reports the error", async () => {
  const fetch = stubFetch();
  fetch
    .mockResolvedValueOnce(Response.json({ enabled: true }))
    .mockResolvedValueOnce(
      Response.json({ error: "History could not be deleted" }, { status: 500 }),
    );
  render(createElement(ListeningHistorySettings, { account: "alice" }));
  await saveSwitch();
  fireEvent.click(
    screen.getByRole("button", { name: "Delete listening history" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Delete history" }));
  await screen.findByRole("alert");
  expect(screen.queryByText(/entries removed/)).toBeNull();
});

test("a switched account retires the earlier account's read", async () => {
  const fetch = stubFetch();
  let release!: (response: Response) => void;
  fetch
    .mockReturnValueOnce(
      new Promise<Response>((resolve) => {
        release = resolve;
      }),
    )
    .mockResolvedValueOnce(Response.json({ enabled: true }));
  const view = render(
    createElement(ListeningHistorySettings, { account: "alice" }),
  );
  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(1));
  const retired = fetch.mock.calls[0][1]?.signal;
  view.rerender(createElement(ListeningHistorySettings, { account: "bob" }));
  expect(retired?.aborted).toBe(true);
  const toggle = await saveSwitch();
  await act(async () => {
    release(Response.json({ enabled: false }));
  });
  expect(isOn(toggle)).toBe(true);
});
