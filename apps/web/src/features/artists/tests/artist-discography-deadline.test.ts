import { createElement } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ArtistDiscography from "../components/ArtistDiscography";
import type { Schema } from "../../../shared/schema";
import {
  artistAssociation,
  artistReviewAccount,
  artistReviewDetail,
  reviewedArtistProfile,
} from "../fixtures/artistReview";
import {
  discographyTitle,
  firstDiscographyPage,
} from "../fixtures/artistDiscography";

function stalledBody(status: number, signal: AbortSignal) {
  let entered!: () => void;
  const reading = new Promise<void>((resolve) => {
    entered = resolve;
  });
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  let retired = false;
  let aborts = 0;
  const stop = () => {
    if (retired) return;
    retired = true;
    controller.error(
      new DOMException("The owned body was aborted", "AbortError"),
    );
  };
  const body = new ReadableStream<Uint8Array>({
    start(value) {
      controller = value;
      controller.enqueue(new TextEncoder().encode("{"));
    },
    pull() {
      entered();
    },
    cancel() {
      retired = true;
    },
  });
  signal.addEventListener(
    "abort",
    () => {
      aborts++;
      stop();
    },
    { once: true },
  );
  return {
    response: new Response(body, {
      status,
      headers: { "content-type": "application/json" },
    }),
    signal,
    reading,
    close: stop,
    retired: () => retired,
    aborts: () => aborts,
  };
}

test("discography reads have a twenty-second deadline through success and error response bodies", async () => {
  vi.useFakeTimers();
  const errors = vi.spyOn(console, "error");
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const path = `/api/v1/recordings/${artistReviewDetail.recording_id}/artist-associations/${artistAssociation.id}/discography`;
  const held: ReturnType<typeof stalledBody>[] = [];
  let reads = 0;
  const fetch = vi.fn(async (requested: string, options?: RequestInit) => {
    if (requested !== path || options?.method !== "POST" || !options.signal)
      throw new Error(
        "Unexpected request outside the owned discography deadline",
      );
    const attempt = reads++;
    if (attempt < 2) {
      const body = stalledBody(attempt === 0 ? 200 : 409, options.signal);
      held.push(body);
      return body.response;
    }
    if (attempt === 2) return Response.json(firstDiscographyPage);
    throw new Error(
      "A timed-out discography read must wait for an explicit retry",
    );
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(
    createElement(ArtistDiscography, {
      account: artistReviewAccount,
      recording: artistReviewDetail.recording_id,
      association: artistAssociation,
      profile: reviewedArtistProfile.profile,
    }),
  );
  try {
    expect(fetch).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Browse discography" }));
    for (const attempt of [0, 1]) {
      await act(async () => {
        await held[attempt].reading;
      });
      const region = screen.getByRole("region", { name: "Discography" });
      expect(held[attempt].response.bodyUsed).toBe(true);
      expect(region.getAttribute("aria-busy")).toBe("true");
      await act(async () => {
        await vi.advanceTimersByTimeAsync(19_999);
      });
      expect(held[attempt].signal.aborted).toBe(false);
      expect(held[attempt].retired()).toBe(false);
      expect(region.getAttribute("aria-busy")).toBe("true");
      expect(within(region).queryByRole("alert")).toBeNull();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1);
      });
      expect(held[attempt].signal.aborted).toBe(true);
      expect(held[attempt].retired()).toBe(true);
      expect(held[attempt].aborts()).toBe(1);
      expect(region.getAttribute("aria-busy")).not.toBe("true");
      expect(within(region).getByRole("alert").textContent).toMatch(
        /timed out|time limit|taking too long/i,
      );
      expect(
        within(region).queryByRole("button", { name: "Restart discography" }),
      ).toBeNull();
      const retry = within(region).getByRole("button", { name: "Try again" });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(60_000);
      });
      expect(fetch).toHaveBeenCalledTimes(attempt + 1);
      expect(
        within(region).queryByRole("heading", { name: discographyTitle }),
      ).toBeNull();
      fireEvent.click(retry);
    }
    await act(async () => {
      for (let step = 0; step < 8; step++) await Promise.resolve();
    });
    const region = screen.getByRole("region", { name: "Discography" });
    expect(
      within(region).getByRole("heading", { name: discographyTitle }),
    ).toBeTruthy();
    expect(within(region).queryByRole("alert")).toBeNull();
    expect(region.getAttribute("aria-busy")).not.toBe("true");
    expect(fetch).toHaveBeenCalledTimes(3);
    for (const [, options] of fetch.mock.calls) {
      expect(JSON.parse(options?.body as string)).toEqual({
        limit: 20,
        continuation: null,
      } satisfies Schema<"ReadArtistDiscography">);
      expect(new Headers(options?.headers).get("x-hito-account")).toBe(
        artistReviewAccount,
      );
      expect(options?.credentials).toBe("same-origin");
    }
    expect(play).not.toHaveBeenCalled();
    expect(errors).not.toHaveBeenCalled();
  } finally {
    await act(async () => {
      view.unmount();
      for (const body of held) body.close();
      await Promise.resolve();
    });
  }
});
