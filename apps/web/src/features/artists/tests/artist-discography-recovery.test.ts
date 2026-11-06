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
  hostDiscographyContinuation,
  lastDiscographyPage,
  providerDiscographyCursor,
} from "../fixtures/artistDiscography";

vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn(), pending: false }),
}));

test("an expired discography continuation requires an explicit restart before replacing its known releases", async () => {
  const errors = vi.spyOn(console, "error");
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const before = structuredClone({
    association: artistAssociation,
    detail: artistReviewDetail,
    profile: reviewedArtistProfile,
  });
  const path = `/api/v1/recordings/${artistReviewDetail.recording_id}/artist-associations/${artistAssociation.id}/discography`;
  const freshTitle = "Sessão depois de nova revisão";
  const fresh = {
    ...lastDiscographyPage,
    page: {
      ...lastDiscographyPage.page,
      items: [
        {
          ...lastDiscographyPage.page.items[0],
          id: "opaque:release-group/novo-catálogo?edição=三",
          title: freshTitle,
        },
      ],
    },
  } satisfies Schema<"ArtistDiscographyAvailability">;
  const responses: Response[] = [];
  const reply = (value: unknown, status = 200) => {
    const response = Response.json(value, { status });
    responses.push(response);
    return response;
  };
  let releaseRestart!: (response: Response) => void;
  const restarted = new Promise<Response>((resolve) => {
    releaseRestart = resolve;
  });
  let reads = 0;
  const fetch = vi.fn(async (requested: string, options?: RequestInit) => {
    if (requested !== path || options?.method !== "POST")
      throw new Error(
        "Unexpected request outside the owned discography recovery",
      );
    reads++;
    if (reads === 1) return reply(firstDiscographyPage);
    if (reads === 2)
      return reply(
        { error: "The catalog continuation changed. Start again." },
        409,
      );
    if (reads === 3) return restarted;
    throw new Error("Discography recovery must wait for an explicit action");
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
    const region = await screen.findByRole("region", { name: "Discography" });
    const list = await within(region).findByRole("list", {
      name: "Discography releases",
    });
    await within(list).findByRole("heading", { name: discographyTitle });
    expect(fetch).toHaveBeenCalledTimes(1);
    fireEvent.click(
      within(region).getByRole("button", { name: "Load more releases" }),
    );
    const error = await within(region).findByRole("alert");
    expect(error.textContent).toMatch(/changed|expired/i);
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    expect(
      within(list).getByRole("heading", { name: discographyTitle }),
    ).toBeTruthy();
    await act(async () => {
      await Promise.resolve();
    });
    expect(fetch).toHaveBeenCalledTimes(2);
    const restart = within(region).getByRole("button", {
      name: "Restart discography",
    });
    expect(
      within(region).queryByRole("button", { name: "Load more releases" }),
    ).toBeNull();
    fireEvent.click(restart);
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(3));
    expect(region.getAttribute("aria-busy")).toBe("true");
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    expect(
      within(list).getByRole("heading", { name: discographyTitle }),
    ).toBeTruthy();
    expect(
      within(list).queryByRole("heading", { name: freshTitle }),
    ).toBeNull();
    expect(
      fetch.mock.calls.map(([, options]) =>
        JSON.parse(options?.body as string),
      ),
    ).toEqual([
      {
        limit: 20,
        continuation: null,
      } satisfies Schema<"ReadArtistDiscography">,
      {
        limit: 20,
        continuation: hostDiscographyContinuation,
      } satisfies Schema<"ReadArtistDiscography">,
      {
        limit: 20,
        continuation: null,
      } satisfies Schema<"ReadArtistDiscography">,
    ]);
    await act(async () => {
      releaseRestart(reply(fresh));
    });
    await within(list).findByRole("heading", { name: freshTitle });
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    expect(
      within(list).queryByRole("heading", { name: discographyTitle }),
    ).toBeNull();
    expect(region.getAttribute("aria-busy")).not.toBe("true");
    expect(within(region).queryByRole("alert")).toBeNull();
    expect(
      within(region).queryByRole("button", { name: "Restart discography" }),
    ).toBeNull();
    expect(
      within(region).queryByRole("button", { name: "Load more releases" }),
    ).toBeNull();
    expect(fetch).toHaveBeenCalledTimes(3);
    for (const [, options] of fetch.mock.calls) {
      const headers = new Headers(options?.headers);
      expect(headers.get("x-hito-account")).toBe(artistReviewAccount);
      expect(headers.get("x-hito-request")).toBe("1");
      expect(options?.credentials).toBe("same-origin");
      expect(options?.signal).toBeInstanceOf(AbortSignal);
      expect(options?.body).not.toContain(providerDiscographyCursor);
    }
    expect(responses.every((response) => response.bodyUsed)).toBe(true);
    expect({
      association: artistAssociation,
      detail: artistReviewDetail,
      profile: reviewedArtistProfile,
    }).toEqual(before);
    expect(play).not.toHaveBeenCalled();
    expect(errors).not.toHaveBeenCalled();
  } finally {
    releaseRestart(Response.json(fresh));
    view.unmount();
  }
});
