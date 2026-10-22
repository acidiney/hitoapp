import { createElement } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { ArtworkProvider } from "./ArtworkProvider";
import Artwork from "./components/Artwork";
import ArtworkStatus from "./components/ArtworkStatus";

test("a mounted image can explicitly retry private bytes after 129 settled targets retire its bounded cache entry", async () => {
  vi.useFakeTimers();
  const ids = Array.from(
    { length: 129 },
    (_, index) => `80000000-0000-4000-8000-${String(index).padStart(12, "0")}`,
  );
  const availability = new Set(
    ids.map((id) => `/api/v1/playlists/${id}/artwork/availability`),
  );
  let active = 0;
  let peak = 0;
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    expect(availability.has(path)).toBe(true);
    expect(options?.method).toBe("GET");
    expect(options?.credentials).toBe("same-origin");
    expect(new Headers(options?.headers).get("x-hito-account")).toBe("alice");
    active++;
    peak = Math.max(peak, active);
    return Promise.resolve(Response.json({ can_fetch: false })).finally(() => {
      active--;
    });
  });
  vi.stubGlobal("fetch", fetch);
  const { container, unmount } = render(
    createElement(ArtworkProvider, {
      account: "alice",
      children: ids.map((id) =>
        createElement(
          "section",
          { key: id, "data-target": id },
          createElement(Artwork, { playlistId: id, playlist: true }),
          createElement(ArtworkStatus, { playlistId: id }),
        ),
      ),
    }),
  );
  const card = (index: number) =>
    container.querySelector<HTMLElement>(`[data-target="${ids[index]}"]`)!;
  const cover = (index: number) => card(index).querySelector("img");
  await act(async () => {
    for (let index = 0; index < 128; index++) fireEvent.error(cover(index)!);
  });
  expect(fetch).toHaveBeenCalledTimes(128);
  expect(screen.getAllByRole("button", { name: "Retry artwork" })).toHaveLength(
    128,
  );
  expect(active).toBe(0);
  expect(cover(128)).toBeTruthy();
  await act(async () => {
    fireEvent.error(cover(128)!);
  });
  expect(fetch).toHaveBeenCalledTimes(129);
  expect(screen.getAllByRole("button", { name: "Retry artwork" })).toHaveLength(
    129,
  );
  const retry = within(card(0)).getByRole("button", { name: "Retry artwork" });
  expect(cover(0)).toBeNull();
  const image = `/api/v1/playlists/${ids[0]}/artwork`;
  await act(async () => {
    fireEvent.click(retry);
  });
  expect(cover(0)?.getAttribute("src")).toBe(image);
  expect(fetch).toHaveBeenCalledTimes(130);
  expect(fetch.mock.calls[129][0]).toBe(`${image}/availability`);
  await act(async () => {
    fireEvent.load(cover(0)!);
  });
  expect(
    within(card(0)).queryByRole("button", { name: "Retry artwork" }),
  ).toBeNull();
  expect(screen.getAllByRole("button", { name: "Retry artwork" })).toHaveLength(
    128,
  );
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch).toHaveBeenCalledTimes(130);
  expect(active).toBe(0);
  expect(peak).toBeLessThanOrEqual(2);
  expect(
    new Set(fetch.mock.calls.slice(0, 129).map(([path]) => path)).size,
  ).toBe(129);
  unmount();
  expect(vi.getTimerCount()).toBe(0);
});
