import type { Schema } from "../../../shared/schema";
import { createElement, Fragment } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { ArtworkProvider } from "../components/ArtworkProvider";
import Artwork from "../components/Artwork";
import ArtworkStatus from "../components/ArtworkStatus";

const id = "3017a834-f2ef-4b57-9411-c3b56781132e";
const targets = [
  {
    kind: "recording",
    collection: "recordings",
    props: { recording: id },
    identity: { recording_id: id },
  },
  {
    kind: "release",
    collection: "releases",
    props: { release: id },
    identity: { release_id: id },
  },
  {
    kind: "playlist",
    collection: "playlists",
    props: { playlistId: id },
    identity: { playlist_id: id },
  },
] as const;

test.each(
  targets.flatMap((target) =>
    (["ready"] as const).map((phase) => ({ ...target, phase })),
  ),
)(
  "a second $kind renderer cannot clear a hidden renderer's $phase failure before explicit Retry",
  async (target) => {
    vi.useFakeTimers();
    const image = `/api/v1/${target.collection}/${id}/artwork`;
    const ready = {
      id: "375ad9c3-72b0-43d1-82ad-0fc00669b52c",
      ...target.identity,
      status: "ready",
      attempts: 1,
      error: null,
      created_at: 1,
      updated_at: 2,
    } satisfies
      | Schema<"ArtworkFetch">
      | Schema<"ReleaseArtworkFetch">
      | Schema<"PlaylistArtworkFetch">;
    const fetch = vi.fn(async (path: string, options?: RequestInit) => {
      if (path === `${image}/availability` && options?.method === "GET")
        return Response.json({ can_fetch: target.phase === "ready" });
      if (
        target.phase === "ready" &&
        path === `${image}/fetch` &&
        options?.method === "POST"
      )
        return Response.json(ready, { status: 202 });
      throw new Error("Unexpected request outside the shared artwork workflow");
    });
    vi.stubGlobal("fetch", fetch);
    const { container, unmount } = render(
      createElement(ArtworkProvider, {
        account: "alice",
        children: createElement(
          Fragment,
          null,
          createElement(Artwork, {
            ...target.props,
            className: "first-renderer",
          }),
          createElement(Artwork, {
            ...target.props,
            className: "second-renderer",
          }),
          createElement(ArtworkStatus, target.props),
        ),
      }),
    );
    const first = () => container.querySelector(".first-renderer img");
    const second = () => container.querySelector(".second-renderer img");
    expect(first()?.getAttribute("src")).toBe(image);
    expect(second()?.getAttribute("src")).toBe(image);
    expect(fetch).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.error(first()!);
    });
    if (target.phase === "ready") {
      expect(first()?.getAttribute("src")).toBe(image);
      await act(async () => {
        fireEvent.error(first()!);
      });
    }
    expect(first()).toBeNull();
    expect(screen.getByRole("button", { name: "Retry artwork" })).toBeTruthy();
    const loadedPeer = second();
    await act(async () => {
      fireEvent.load(loadedPeer!);
    });
    expect(first()).toBeNull();
    expect(second()).toBe(loadedPeer);
    const retry = screen.getByRole("button", { name: "Retry artwork" });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(fetch).toHaveBeenCalledTimes(target.phase === "ready" ? 2 : 1);
    await act(async () => {
      fireEvent.click(retry);
    });
    expect(first()?.getAttribute("src")).toBe(image);
    expect(second()?.getAttribute("src")).toBe(image);
    expect(second()).not.toBe(loadedPeer);
    expect(fetch).toHaveBeenCalledTimes(2);
    await act(async () => {
      fireEvent.load(first()!);
      fireEvent.load(second()!);
    });
    expect(screen.queryByRole("button", { name: "Retry artwork" })).toBeNull();
    await act(async () => {
      fireEvent.error(first()!);
      fireEvent.load(second()!);
    });
    expect(first()).toBeNull();
    expect(second()?.getAttribute("src")).toBe(image);
    expect(screen.getByRole("button", { name: "Retry artwork" })).toBeTruthy();
    expect(fetch).toHaveBeenCalledTimes(2);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  },
);

test.each(targets)(
  "an initial $kind image failure stays inside its account generation when the same private URL is revisited",
  async (target) => {
    vi.useFakeTimers();
    const image = `/api/v1/${target.collection}/${id}/artwork`;
    const fetch = vi.fn(async (path: string, options?: RequestInit) => {
      expect(path).toBe(`${image}/availability`);
      expect(new Headers(options?.headers).get("x-hito-account")).toBe("alice");
      return Response.json({ can_fetch: false });
    });
    vi.stubGlobal("fetch", fetch);
    const view = (account: string) =>
      createElement(ArtworkProvider, {
        account,
        children: createElement(
          Fragment,
          null,
          createElement(Artwork, target.props),
          createElement(ArtworkStatus, target.props),
        ),
      });
    const { container, rerender, unmount } = render(view("alice"));
    const cover = () => container.querySelector(".artwork img");
    await act(async () => {
      fireEvent.error(cover()!);
    });
    expect(cover()).toBeNull();
    expect(screen.queryByRole("button", { name: "Retry artwork" })).toBeNull();
    const oldSignal = fetch.mock.calls[0][1]?.signal;
    rerender(view("bob"));
    expect(cover()?.getAttribute("src")).toBe(image);
    expect(screen.queryByRole("button", { name: "Retry artwork" })).toBeNull();
    expect(oldSignal?.aborted).toBe(true);
    await act(async () => {
      fireEvent.load(cover()!);
    });
    rerender(view("alice"));
    expect(cover()?.getAttribute("src")).toBe(image);
    expect(screen.queryByRole("button", { name: "Retry artwork" })).toBeNull();
    expect(fetch).toHaveBeenCalledTimes(1);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  },
);

test.each(targets)(
  "a ready $kind receipt exposes image load failure and explicitly retries the private image without another job",
  async (target) => {
    vi.useFakeTimers();
    const image = `/api/v1/${target.collection}/${id}/artwork`;
    const ready = {
      id: "46aa38b1-980d-44f0-a33d-0346e771058d",
      ...target.identity,
      status: "ready",
      attempts: 1,
      error: null,
      created_at: 1,
      updated_at: 2,
    } satisfies
      | Schema<"ArtworkFetch">
      | Schema<"ReleaseArtworkFetch">
      | Schema<"PlaylistArtworkFetch">;
    const fetch = vi.fn(async (path: string, options?: RequestInit) => {
      if (path === `${image}/availability` && options?.method === "GET")
        return Response.json({ can_fetch: true });
      if (path === `${image}/fetch` && options?.method === "POST")
        return Response.json(ready, { status: 202 });
      throw new Error("Unexpected request outside this typed artwork workflow");
    });
    vi.stubGlobal("fetch", fetch);
    const view = (account: string) =>
      createElement(ArtworkProvider, {
        account,
        children: createElement(
          Fragment,
          null,
          createElement(Artwork, target.props),
          createElement(ArtworkStatus, target.props),
        ),
      });
    const { container, rerender, unmount } = render(view("alice"));
    const cover = () => container.querySelector(".artwork img");
    await act(async () => {
      fireEvent.error(cover()!);
    });
    expect(cover()?.getAttribute("src")).toBe(image);
    expect(fetch).toHaveBeenCalledTimes(2);
    const failed = cover();
    await act(async () => {
      fireEvent.error(failed!);
    });
    expect(cover()).toBeNull();
    expect(screen.getByText("Artwork could not be loaded.")).toBeTruthy();
    const retry = screen.getByRole("button", { name: "Retry artwork" });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(fetch).toHaveBeenCalledTimes(2);
    await act(async () => {
      fireEvent.click(retry);
    });
    expect(cover()?.getAttribute("src")).toBe(image);
    expect(cover()).not.toBe(failed);
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("button", { name: "Retry artwork" })).toBeNull();
    await act(async () => {
      fireEvent.load(cover()!);
    });
    await act(async () => {
      fireEvent.error(cover()!);
    });
    expect(screen.getByRole("button", { name: "Retry artwork" })).toBeTruthy();
    rerender(view("bob"));
    expect(cover()?.getAttribute("src")).toBe(image);
    expect(screen.queryByRole("button", { name: "Retry artwork" })).toBeNull();
    expect(fetch).toHaveBeenCalledTimes(2);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  },
);
