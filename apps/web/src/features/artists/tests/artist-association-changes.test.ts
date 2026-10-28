import { createElement } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ArtistReview from "../components/ArtistReview";
import type { components } from "../../../generated";
import { unselectedArtistIdentity } from "../fixtures/artistIdentity";
import {
  artistAssociation,
  artistReviewAccount,
  artistReviewConnections,
  artistReviewDetail,
  artistReviewSettings,
} from "../fixtures/artistReview";

type Schema = components["schemas"];

function owned(options: { failFirstUnlink?: boolean } = {}) {
  const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
  const associations = `${base}/artist-associations`;
  let linked = true;
  let revision = 1;
  let failures = options.failFirstUnlink ? 1 : 0;
  const bodies: Record<string, unknown>[] = [];
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === `${base}/artist-identity`)
      return Response.json(unselectedArtistIdentity);
    if (method === "GET" && path === "/api/v1/metadata/settings")
      return Response.json(artistReviewSettings);
    if (method === "GET" && path === "/api/v1/plugins")
      return Response.json(artistReviewConnections);
    if (method === "GET" && path === associations)
      return Response.json(linked ? [artistAssociation] : []);
    if (method === "GET" && path === `${base}/artist-association-changes`)
      return Response.json({
        recording_id: artistReviewDetail.recording_id,
        revision,
        undo: null,
      } satisfies Schema["ArtistAssociationChanges"]);
    if (method === "POST" && path.startsWith(`${associations}/`)) {
      const body = JSON.parse(options?.body as string);
      bodies.push(body);
      if (path.endsWith("/unlink") && failures > 0) {
        failures -= 1;
        throw new TypeError("network lost");
      }
      expect(body.expected_revision).toBe(revision);
      revision += 1;
      linked = path.endsWith("/undo") ? !linked : false;
      return Response.json({
        request_id: body.request_id,
        recording_id: artistReviewDetail.recording_id,
        revision,
        association_id: artistAssociation.id,
        linked,
      } satisfies Schema["ArtistAssociationChangeReceipt"]);
    }
    throw new Error("Unexpected HTTP request outside the association changes");
  });
  vi.stubGlobal("fetch", fetch);
  return { fetch, bodies, associations };
}

async function open() {
  render(
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: artistReviewDetail,
    }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Find artist information" }),
  );
  return screen.findByRole("region", { name: "Saved artist reviews" });
}

test("unlinking a saved artist review removes it and undo restores it without provider requests", async () => {
  const { fetch, bodies, associations } = owned();
  const saved = await open();
  expect(
    within(saved).getByRole("button", { name: "View saved artist review 1" }),
  ).toBeTruthy();
  expect(screen.queryByRole("button", { name: /Undo last/ })).toBeNull();

  fireEvent.click(
    within(saved).getByRole("button", { name: "Unlink saved artist review 1" }),
  );
  const undo = await screen.findByRole("button", {
    name: "Undo last artist review change",
  });
  expect(bodies).toHaveLength(1);
  expect(bodies[0]).toEqual({
    request_id: expect.any(String),
    expected_revision: 1,
    association_id: artistAssociation.id,
  } satisfies Schema["UnlinkRecordingArtist"] & Record<string, unknown>);
  expect(
    screen.queryByRole("button", { name: /View saved artist review/ }),
  ).toBeNull();

  fireEvent.click(undo);
  await screen.findByRole("button", { name: "View saved artist review 1" });
  expect(bodies[1]).toEqual({
    request_id: expect.any(String),
    expected_revision: 2,
  } satisfies Schema["UndoRecordingArtistChange"]);
  expect(bodies[1].request_id).not.toBe(bodies[0].request_id);
  expect(
    fetch.mock.calls.filter(([path]) => path.endsWith("/profile")),
  ).toHaveLength(0);
  expect(
    fetch.mock.calls
      .filter(([, options]) => options?.method === "POST")
      .map(([path]) => path),
  ).toEqual([`${associations}/unlink`, `${associations}/undo`]);
});

test("a lost unlink acknowledgement is recovered by retrying the exact same request", async () => {
  const { bodies } = owned({ failFirstUnlink: true });
  const saved = await open();
  fireEvent.click(
    within(saved).getByRole("button", { name: "Unlink saved artist review 1" }),
  );
  const retry = await screen.findByRole("button", {
    name: "Retry the same artist review change request",
  });
  expect(screen.getByRole("alert")).toBeTruthy();
  fireEvent.click(retry);
  await waitFor(() =>
    expect(
      screen.queryByRole("button", { name: /View saved artist review/ }),
    ).toBeNull(),
  );
  expect(bodies).toHaveLength(2);
  expect(bodies[1]).toEqual(bodies[0]);
});
