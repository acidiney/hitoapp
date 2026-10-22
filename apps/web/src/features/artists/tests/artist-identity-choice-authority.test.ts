import { createElement } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ArtistReview from "../components/ArtistReview";
import type { components } from "../../../generated";
import {
  artistAssociation,
  artistReviewAccount,
  artistReviewConnections,
  artistReviewContext,
  artistReviewDetail,
  artistReviewSettings,
  reviewedArtistProfile,
} from "../fixtures/artistReview";
import {
  artistIdentityGroup,
  selectedArtistIdentity,
  unselectedArtistIdentity,
} from "../fixtures/artistIdentity";

type Schema = components["schemas"];

test("the current-choice deadline releases its owner and cancels late headers without exposing a choice", async () => {
  vi.useFakeTimers();
  const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
  const signals: AbortSignal[] = [];
  let release!: (response: Response) => void;
  const held = new Promise<Response>((resolve) => {
    release = resolve;
  });
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === "/api/v1/metadata/settings")
      return Response.json(artistReviewSettings);
    if (method === "GET" && path === "/api/v1/plugins")
      return Response.json(artistReviewConnections);
    if (method === "GET" && path === `${base}/artist-associations`)
      return Response.json([]);
    if (method === "GET" && path === `${base}/artist-identity`) {
      signals.push(options?.signal as AbortSignal);
      return held;
    }
    throw new Error(
      "Unexpected request outside the owned artist choice deadline",
    );
  });
  vi.stubGlobal("fetch", fetch);
  render(
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: artistReviewDetail,
    }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Find artist information" }),
  );
  await act(async () => {
    await Promise.resolve();
  });
  expect(signals).toHaveLength(1);
  expect(signals[0].aborted).toBe(false);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(15000);
  });
  expect(signals[0].aborted).toBe(true);
  expect(screen.getByRole("alert")).toBeTruthy();
  expect(
    screen.getByRole("button", { name: "Check current artist choice" }),
  ).toBeTruthy();
  expect(
    screen
      .getByRole("region", { name: "Artist choice for this track" })
      .getAttribute("aria-busy"),
  ).toBe("false");
  const calls = fetch.mock.calls.length;
  let cancelled = false;
  await act(async () => {
    release(
      new Response(
        new ReadableStream<Uint8Array>({
          cancel() {
            cancelled = true;
          },
        }),
        { headers: { "content-type": "application/json" } },
      ),
    );
  });
  expect(cancelled).toBe(true);
  expect(fetch.mock.calls).toHaveLength(calls);
  expect(
    screen.queryByRole("button", { name: "Choose this reviewed artist" }),
  ).toBeNull();
  expect(
    screen.queryByText("Reviewed artist selected for this track.", {
      exact: true,
    }),
  ).toBeNull();
});

test("a malformed acknowledgement cannot authorize a current read or replace the pending exact request", async () => {
  const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
  let written = false;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === "/api/v1/metadata/settings")
      return Response.json(artistReviewSettings);
    if (method === "GET" && path === "/api/v1/plugins")
      return Response.json(artistReviewConnections);
    if (method === "GET" && path === `${base}/artist-associations`)
      return Response.json([artistAssociation]);
    if (
      method === "GET" &&
      path === `${base}/artist-associations/${artistAssociation.id}/profile`
    )
      return Response.json(reviewedArtistProfile);
    if (method === "GET" && path === `${base}/artist-identity`)
      return Response.json(
        written ? selectedArtistIdentity : unselectedArtistIdentity,
      );
    if (method === "POST" && path === `${base}/artist-review/context`)
      return Response.json(artistReviewContext);
    if (method === "PUT" && path === `${base}/artist-identity`) {
      written = true;
      const body = JSON.parse(
        options?.body as string,
      ) as Schema["ChooseRecordingArtistIdentity"];
      return Response.json({
        request_id: body.request_id,
        recording_id: artistReviewDetail.recording_id,
        revision: 1,
        selection: { association_id: artistAssociation.id, group_id: 42 },
      });
    }
    throw new Error(
      "Unexpected request outside artist identity receipt validation",
    );
  });
  vi.stubGlobal("fetch", fetch);
  render(
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: artistReviewDetail,
    }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Find artist information" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "View saved artist review 1" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose this reviewed artist" }),
  );
  await waitFor(() => expect(written).toBe(true));
  expect(
    await screen.findByRole("button", {
      name: "Retry the same artist choice request",
    }),
  ).toBeTruthy();
  expect(
    screen.queryByText("Reviewed artist selected for this track.", {
      exact: true,
    }),
  ).toBeNull();
  expect(
    fetch.mock.calls.filter(
      ([path, options]) =>
        path === `${base}/artist-identity` &&
        (options?.method ?? "GET") === "GET",
    ),
  ).toHaveLength(1);
});

test.each(["account", "metadata", "recording"] as const)(
  "%s retirement aborts the old choice and cancels a late response without publishing its acknowledgement",
  async (retirement) => {
    const errors = vi.spyOn(console, "error");
    const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
    const writes: AbortSignal[] = [];
    let release!: (response: Response) => void;
    const held = new Promise<Response>((resolve) => {
      release = resolve;
    });
    let currentDetail = structuredClone(artistReviewDetail);
    let currentAccount = artistReviewAccount;
    const fetch = vi.fn(async (path: string, options?: RequestInit) => {
      const method = options?.method ?? "GET";
      const currentBase = `/api/v1/recordings/${currentDetail.recording_id}`;
      if (method === "GET" && path === "/api/v1/metadata/settings")
        return Response.json(artistReviewSettings);
      if (method === "GET" && path === "/api/v1/plugins")
        return Response.json(artistReviewConnections);
      if (method === "GET" && path === `${currentBase}/artist-associations`)
        return Response.json(
          currentAccount === artistReviewAccount ? [artistAssociation] : [],
        );
      if (
        method === "GET" &&
        path === `${base}/artist-associations/${artistAssociation.id}/profile`
      )
        return Response.json(reviewedArtistProfile);
      if (method === "GET" && path === `${currentBase}/artist-identity`)
        return Response.json({
          ...unselectedArtistIdentity,
          recording_id: currentDetail.recording_id,
        });
      if (method === "POST" && path === `${base}/artist-review/context`)
        return Response.json(artistReviewContext);
      if (method === "PUT" && path === `${base}/artist-identity`) {
        writes.push(options?.signal as AbortSignal);
        return held;
      }
      throw new Error("Unexpected request outside artist identity retirement");
    });
    vi.stubGlobal("fetch", fetch);
    const view = render(
      createElement(ArtistReview, {
        account: currentAccount,
        detail: currentDetail,
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Find artist information" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "View saved artist review 1" }),
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Choose this reviewed artist",
      }),
    );
    await waitFor(() => expect(writes).toHaveLength(1));
    if (retirement === "account") currentAccount = "bob";
    if (retirement === "metadata")
      currentDetail = {
        ...currentDetail,
        artist: {
          ...currentDetail.artist,
          operation_id: "80000000-0000-4000-8000-000000000002",
          value: "Changed saved artist credit",
        },
      };
    if (retirement === "recording")
      currentDetail = {
        ...currentDetail,
        recording_id: "80000000-0000-4000-8000-000000000003",
      };
    view.rerender(
      createElement(ArtistReview, {
        account: currentAccount,
        detail: currentDetail,
      }),
    );
    await waitFor(() => expect(writes[0].aborted).toBe(true));
    fireEvent.click(
      screen.getByRole("button", { name: "Find artist information" }),
    );
    await screen.findByText("No reviewed artist selected for this track.", {
      exact: true,
    });
    const callsBeforeRelease = fetch.mock.calls.length;
    let cancelled = false;
    release(
      new Response(
        new ReadableStream<Uint8Array>({
          cancel() {
            cancelled = true;
          },
        }),
        { headers: { "content-type": "application/json" } },
      ),
    );
    await waitFor(() => expect(cancelled).toBe(true));
    expect(fetch.mock.calls).toHaveLength(callsBeforeRelease);
    expect(
      screen.getByText("No reviewed artist selected for this track.", {
        exact: true,
      }),
    ).toBeTruthy();
    expect(
      screen.queryByText("Reviewed artist selected for this track.", {
        exact: true,
      }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", {
        name: "Retry the same artist choice request",
      }),
    ).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(errors).not.toHaveBeenCalled();
  },
);

test("a valid acknowledgement cannot leave the old choice writable when the required current read returns another recording", async () => {
  const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
  let written = false;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === "/api/v1/metadata/settings")
      return Response.json(artistReviewSettings);
    if (method === "GET" && path === "/api/v1/plugins")
      return Response.json(artistReviewConnections);
    if (method === "GET" && path === `${base}/artist-associations`)
      return Response.json([artistAssociation]);
    if (
      method === "GET" &&
      path === `${base}/artist-associations/${artistAssociation.id}/profile`
    )
      return Response.json(reviewedArtistProfile);
    if (method === "GET" && path === `${base}/artist-identity`)
      return Response.json(
        written
          ? {
              ...selectedArtistIdentity,
              recording_id: "80000000-0000-4000-8000-000000000001",
            }
          : unselectedArtistIdentity,
      );
    if (method === "POST" && path === `${base}/artist-review/context`)
      return Response.json(artistReviewContext);
    if (method === "PUT" && path === `${base}/artist-identity`) {
      written = true;
      const body = JSON.parse(
        options?.body as string,
      ) as Schema["ChooseRecordingArtistIdentity"];
      return Response.json({
        request_id: body.request_id,
        recording_id: artistReviewDetail.recording_id,
        revision: 1,
        selection: {
          association_id: artistAssociation.id,
          group_id: artistIdentityGroup,
        },
      } satisfies Schema["ArtistIdentityChoiceReceipt"]);
    }
    throw new Error(
      "Unexpected request outside current artist choice validation",
    );
  });
  vi.stubGlobal("fetch", fetch);
  render(
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: artistReviewDetail,
    }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Find artist information" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "View saved artist review 1" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose this reviewed artist" }),
  );
  await screen.findByRole("alert");
  expect(written).toBe(true);
  expect(
    screen.queryByRole("button", { name: "Choose this reviewed artist" }),
  ).toBeNull();
  expect(
    screen.queryByRole("button", {
      name: "Reset artist choice for this track",
    }),
  ).toBeNull();
  expect(
    screen.getByRole("button", { name: "Check current artist choice" }),
  ).toBeTruthy();
  expect(
    screen.queryByText("Reviewed artist selected for this track.", {
      exact: true,
    }),
  ).toBeNull();
  await waitFor(() =>
    expect(
      fetch.mock.calls.filter(
        ([path, options]) =>
          path === `${base}/artist-identity` &&
          (options?.method ?? "GET") === "GET",
      ),
    ).toHaveLength(2),
  );
});
