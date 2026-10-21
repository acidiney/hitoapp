// @vitest-environment jsdom
import { createElement, type AnchorHTMLAttributes } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistIdentityGroupPage from "./pages/ArtistIdentityGroupPage";
import { artistIdentityBrowsing } from "./fixtures/artistIdentityBrowsing";
import { artistGroupHttp } from "./fixtures/artistGroupDiscography";
import {
  artistAssociation,
  reviewedArtistProfile,
} from "./fixtures/artistReview";
import { firstDiscographyPage } from "./fixtures/artistDiscography";
import type { ArtistIdentityGroupPageProps } from "./types";

vi.mock("./player/PlaybackProvider", () => ({
  usePlayer: () => ({
    playQueue: vi.fn(),
    subscribe: () => () => {},
    snapshot: () => ({ current: null }),
  }),
}));
vi.mock("./hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn(), pending: false }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { get: vi.fn() },
  usePage: () => ({ url: "/" }),
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  localStorage.clear();
  sessionStorage.clear();
});

test.each([
  ["associations", "headers", 200],
  ["associations", "body", 200],
  ["profile", "body", 200],
  ["profile", "body", 409],
] as const)(
  "the loader deadline covers %s %s with HTTP %s",
  async (stage, mode, status) => {
    vi.useFakeTimers();
    const { detail } = artistIdentityBrowsing();
    const errors = vi.spyOn(console, "error");
    const http = artistGroupHttp();
    const held = http.hold(
      stage,
      mode,
      status === 409
        ? { error: "Owned conflict" }
        : stage === "associations"
          ? [artistAssociation]
          : reviewedArtistProfile,
      status,
    );
    const view = render(createElement(ArtistIdentityGroupPage, detail));
    try {
      fireEvent.click(
        screen.getByRole("button", { name: "Browse full discography" }),
      );
      await act(async () => {
        await held.entered;
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(19_999);
      });
      expect(held.signal()?.aborted).toBe(false);
      expect(screen.queryByRole("alert")).toBeNull();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1);
      });
      expect(held.signal()?.aborted).toBe(true);
      expect(held.aborts()).toBe(1);
      expect(screen.getByRole("alert").textContent).toContain("timed out");
      const before = { ...http.calls };
      await act(async () => {
        held.release();
        await vi.advanceTimersByTimeAsync(60_000);
      });
      await flush();
      expect(http.calls).toEqual(before);
      expect(http.calls.discography).toBe(0);
      fireEvent.click(screen.getByRole("button", { name: "Try again" }));
      await flush();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      await flush();
      expect(http.calls.discography).toBe(1);
      expect(
        screen.getByRole("list", { name: "Discography releases" }),
      ).toBeTruthy();
      expect(screen.queryByRole("alert")).toBeNull();
      expect(errors).not.toHaveBeenCalled();
    } finally {
      view.unmount();
      http.close();
    }
  },
);

async function flush() {
  await act(async () => {
    for (let step = 0; step < 8; step++) await Promise.resolve();
  });
}

test.each([
  "account",
  "group",
  "recording",
  "artist",
  "association",
  "provider",
  "provider_artist",
  "credit",
] as const)(
  "a changed %s clears its loader and fences late profile JSON",
  async (field) => {
    const { detail } = artistIdentityBrowsing();
    const errors = vi.spyOn(console, "error");
    const http = artistGroupHttp();
    const held = http.hold("profile", "body", reviewedArtistProfile);
    const view = render(createElement(ArtistIdentityGroupPage, detail));
    try {
      fireEvent.click(
        screen.getByRole("button", { name: "Browse full discography" }),
      );
      await act(async () => {
        await held.entered;
      });
      const changed: ArtistIdentityGroupPageProps = structuredClone(detail);
      switch (field) {
        case "account":
          changed.user.id = "another-account";
          break;
        case "group":
          changed.detail.group.id = "another-chosen-group";
          break;
        case "recording":
          changed.detail.group.name.recording_id = "another-recording";
          break;
        case "artist":
          changed.detail.group.name.artist_id = "another-credit";
          break;
        case "association":
          changed.detail.group.name.association_id = "another-association";
          break;
        case "provider":
          changed.detail.group.provider = { kind: "builtin_musicbrainz" };
          break;
        case "provider_artist":
          changed.detail.group.provider_artist_id = "another-provider-artist";
          break;
        case "credit":
          changed.detail.group.name.value = "Another saved credit";
          break;
      }
      view.rerender(createElement(ArtistIdentityGroupPage, changed));
      expect(held.signal()?.aborted).toBe(true);
      expect(held.aborts()).toBe(1);
      expect(
        screen.getByRole("button", { name: "Browse full discography" }),
      ).toBeTruthy();
      await act(async () => {
        held.release();
      });
      await flush();
      expect(http.calls).toEqual({
        associations: 1,
        profile: 1,
        discography: 0,
      });
      expect(screen.queryByRole("region", { name: "Discography" })).toBeNull();
      expect(screen.queryByRole("alert")).toBeNull();
      expect(errors).not.toHaveBeenCalled();
    } finally {
      view.unmount();
      http.close();
    }
  },
);

test.each([
  ["associations", "headers", "account"],
  ["profile", "headers", "unmount"],
  ["discography", "headers", "unmount"],
  ["discography", "body", "anchor"],
] as const)(
  "late %s %s cannot continue after %s",
  async (stage, mode, change) => {
    const { detail } = artistIdentityBrowsing();
    const errors = vi.spyOn(console, "error");
    const http = artistGroupHttp();
    const held = http.hold(
      stage,
      mode,
      stage === "associations"
        ? [artistAssociation]
        : stage === "profile"
          ? reviewedArtistProfile
          : firstDiscographyPage,
    );
    const view = render(createElement(ArtistIdentityGroupPage, detail));
    try {
      fireEvent.click(
        screen.getByRole("button", { name: "Browse full discography" }),
      );
      if (stage === "discography")
        await waitFor(() => expect(http.calls.discography).toBe(1));
      await act(async () => {
        await held.entered;
      });
      const before = { ...http.calls };
      if (change === "unmount") view.unmount();
      else {
        const changed: ArtistIdentityGroupPageProps = structuredClone(detail);
        if (change === "account") changed.user.id = "another-account";
        else changed.detail.group.name.association_id = "another-association";
        view.rerender(createElement(ArtistIdentityGroupPage, changed));
        expect(
          screen.getByRole("button", { name: "Browse full discography" }),
        ).toBeTruthy();
      }
      expect(held.signal()?.aborted).toBe(true);
      expect(held.aborts()).toBe(1);
      await act(async () => {
        held.release();
      });
      await flush();
      expect(http.calls).toEqual(before);
      expect(
        screen.queryByRole("list", { name: "Discography releases" }),
      ).toBeNull();
      expect(screen.queryByRole("alert")).toBeNull();
      expect(errors).not.toHaveBeenCalled();
    } finally {
      view.unmount();
      http.close();
    }
  },
);
