import { createElement } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ArtistEditions from "./components/metadata/ArtistEditions";
import { discographyHttp } from "./fixtures/artistDiscographyHttp";
import {
  bonusTrackList,
  editionGroup,
  lastEditionPage,
} from "./fixtures/artistEditions";
import {
  artistAssociation,
  artistReviewAccount,
  artistReviewDetail,
  reviewedArtistProfile,
} from "./fixtures/artistReview";
import { log } from "./shared/log";
import { flush } from "./test/async";

test.each(["editions", "track list"] as const)(
  "%s reads stop at twenty seconds through headers, success bodies and error bodies",
  async (phase) => {
    vi.useFakeTimers();
    const errors = vi.spyOn(console, "error");
    const warn = vi.spyOn(log, "warn");
    const http = discographyHttp();
    if (phase === "track list") http.reply(lastEditionPage);
    const accepted = phase === "editions" ? lastEditionPage : bonusTrackList;
    const held = [
      http.hold("headers", accepted),
      http.hold("body", accepted),
      http.hold("body", { error: "Private late continuation failure" }, 409),
    ];
    http.reply(accepted);
    const view = render(
      createElement(ArtistEditions, {
        account: artistReviewAccount,
        recording: artistReviewDetail.recording_id,
        association: artistAssociation,
        profile: reviewedArtistProfile.profile,
        group: editionGroup,
      }),
    );
    const endpoint =
      phase === "editions" ? "release-group-editions" : "edition-track-list";
    const reads = () =>
      http.fetch.mock.calls.filter(([path]) => path.endsWith(`/${endpoint}`));
    try {
      fireEvent.click(screen.getByRole("button", { name: "Browse editions" }));
      if (phase === "track list") {
        await flush();
        fireEvent.click(screen.getByRole("button", { name: "Choose edition" }));
      }
      for (let attempt = 0; attempt < held.length; attempt++) {
        await act(async () => {
          await held[attempt].entered;
        });
        const region = screen.getByRole("region", {
          name:
            phase === "editions" ? "Editions" : "Selected edition track list",
        });
        expect(region.getAttribute("aria-busy")).toBe("true");
        if (attempt > 0) expect(held[attempt].response.bodyUsed).toBe(true);
        await act(async () => {
          await vi.advanceTimersByTimeAsync(19_999);
        });
        expect(held[attempt].signal()?.aborted).toBe(false);
        expect(within(region).queryByRole("alert")).toBeNull();
        await act(async () => {
          await vi.advanceTimersByTimeAsync(1);
        });
        expect(held[attempt].signal()?.aborted).toBe(true);
        expect(held[attempt].aborts()).toBe(1);
        expect(region.getAttribute("aria-busy")).not.toBe("true");
        expect(within(region).getByRole("alert").textContent).toMatch(
          /timed out/i,
        );
        expect(
          screen.queryByRole("button", { name: "Restart editions" }),
        ).toBeNull();
        await act(async () => {
          await vi.advanceTimersByTimeAsync(60_000);
        });
        expect(reads()).toHaveLength(attempt + 1);
        fireEvent.click(
          within(region).getByRole("button", {
            name:
              phase === "editions"
                ? "Try editions again"
                : "Try track list again",
          }),
        );
      }
      await flush();
      expect(reads()).toHaveLength(4);
      expect(screen.queryByRole("alert")).toBeNull();
      if (phase === "editions")
        expect(screen.getByText("Bonus edition", { exact: true })).toBeTruthy();
      else
        expect(
          screen.getByRole("list", { name: "Disc 2 tracks" }),
        ).toBeTruthy();
      const completed = warn.mock.calls.filter(
        ([event]) =>
          event ===
          (phase === "editions"
            ? "artist_editions.finished"
            : "artist_edition_track_list.finished"),
      );
      expect(completed).toHaveLength(3);
      for (const [, fields] of completed)
        expect(fields).toEqual({
          request_id: expect.any(String),
          reason: "timed_out",
          count: 0,
          duration_ms: expect.any(Number),
        });
      expect(JSON.stringify(warn.mock.calls)).not.toContain(
        "Private late continuation failure",
      );
      expect(errors).not.toHaveBeenCalled();
    } finally {
      await act(async () => {
        view.unmount();
        http.close();
      });
      await flush();
    }
    expect(http.responses.every((response) => response.bodyUsed)).toBe(true);
  },
);
