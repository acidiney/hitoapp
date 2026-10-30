import { createElement, type AnchorHTMLAttributes } from "react";
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
  artistReviewAccount,
  artistReviewConnections,
  artistReviewDetail,
  artistReviewSettings,
} from "../fixtures/artistReview";
import {
  artistIdentityGroup,
  artistIdentityGroupDetail,
  selectedArtistIdentity,
  unselectedArtistIdentity,
} from "../fixtures/artistIdentity";
import { deferred } from "../../../test/async";

vi.mock("@inertiajs/react", () => ({
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

function navigationFixture(
  group: () => Response | Promise<Response>,
  resetResponse?: () => Response | Promise<Response>,
) {
  const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
  let reset = false;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    expect(new Headers(options?.headers).get("x-hito-account")).toBe(
      artistReviewAccount,
    );
    if (method === "GET" && path === "/api/v1/metadata/settings")
      return Response.json(artistReviewSettings);
    if (method === "GET" && path === "/api/v1/plugins")
      return Response.json(artistReviewConnections);
    if (method === "GET" && path === `${base}/artist-associations`)
      return Response.json([]);
    if (method === "GET" && path === `${base}/artist-identity`)
      return Response.json(
        reset
          ? { ...unselectedArtistIdentity, revision: 2 }
          : selectedArtistIdentity,
      );
    if (
      method === "GET" &&
      path === `/api/v1/artist-identity-groups/${artistIdentityGroup}?limit=1`
    )
      return group();
    if (method === "POST" && path === `${base}/artist-identity/reset`) {
      const body: components["schemas"]["ResetRecordingArtistIdentity"] =
        JSON.parse(String(options?.body));
      expect(body.expected_revision).toBe(1);
      if (resetResponse) return resetResponse();
      reset = true;
      return Response.json({
        recording_id: artistReviewDetail.recording_id,
        request_id: body.request_id,
        revision: 2,
        selection: null,
      } satisfies components["schemas"]["ArtistIdentityChoiceReceipt"]);
    }
    throw new Error("Unexpected request outside owned artist navigation");
  });
  vi.stubGlobal("fetch", fetch);
  const props = {
    account: artistReviewAccount,
    detail: structuredClone(artistReviewDetail),
  };
  const view = render(createElement(ArtistReview, props));
  fireEvent.click(screen.getByRole("button", { name: "Change artist" }));
  return { fetch, view, props };
}

test("only a current selected choice with its matching saved group read exposes navigation", async () => {
  const held = deferred<Response>();
  let reached = false;
  const { fetch } = navigationFixture(() => {
    reached = true;
    return held.promise;
  });
  await waitFor(() => expect(reached).toBe(true));
  expect(screen.queryByRole("link", { name: "Open chosen artist" })).toBeNull();
  await act(async () => held.resolve(Response.json(artistIdentityGroupDetail)));
  const link = await screen.findByRole("link", { name: "Open chosen artist" });
  expect(link.getAttribute("href")).toBe(
    `/artist-groups/${artistIdentityGroup}`,
  );
  expect(
    screen.getByText(
      `Saved credit for this group: ${artistIdentityGroupDetail.group.name.value}`,
    ),
  ).toBeTruthy();
  expect(
    fetch.mock.calls.every(
      ([, options]) => (options?.method ?? "GET") === "GET",
    ),
  ).toBe(true);
  fireEvent.click(
    screen.getByRole("button", { name: "Reset artist choice for this track" }),
  );
  expect(screen.queryByRole("link", { name: "Open chosen artist" })).toBeNull();
  await screen.findByText("No reviewed artist selected for this track.");
  expect(screen.queryByRole("link", { name: "Open chosen artist" })).toBeNull();
});

test.each(["wrong group", "unavailable group"])(
  "a %s response cannot expose a chosen-artist link",
  async (failure) => {
    const { fetch } = navigationFixture(() =>
      failure === "wrong group"
        ? Response.json({
            ...artistIdentityGroupDetail,
            group: { ...artistIdentityGroupDetail.group, id: "another-group" },
          })
        : Response.json({ error: "Owned unavailable group" }, { status: 503 }),
    );
    await waitFor(() =>
      expect(
        fetch.mock.calls.some(([path]) =>
          path.includes("/artist-identity-groups/"),
        ),
      ).toBe(true),
    );
    await waitFor(() =>
      expect(
        screen
          .getByRole("region", { name: "Artist choice for this track" })
          .getAttribute("aria-busy"),
      ).toBe("false"),
    );
    expect(
      screen.queryByRole("link", { name: "Open chosen artist" }),
    ).toBeNull();
    expect(screen.queryByText(/Saved credit for this group:/)).toBeNull();
  },
);

test.each(["account", "recording", "artist operation", "metadata observation"])(
  "%s changes retire an existing chosen-artist link",
  async (owner) => {
    const { view, props } = navigationFixture(() =>
      Response.json(artistIdentityGroupDetail),
    );
    await screen.findByRole("link", { name: "Open chosen artist" });
    const next = structuredClone(props);
    if (owner === "account") next.account = "other-account";
    if (owner === "recording") next.detail.recording_id = "other-recording";
    if (owner === "artist operation")
      next.detail.artist.operation_id = "other-artist-operation";
    if (owner === "metadata observation")
      next.detail.selected_observation!.id = "other-observation";
    view.rerender(createElement(ArtistReview, next));
    expect(
      screen.queryByRole("link", { name: "Open chosen artist" }),
    ).toBeNull();
    expect(screen.queryByText(/Saved credit for this group:/)).toBeNull();
  },
);

test("a late saved-group read cannot restore navigation after its account retires", async () => {
  const held = deferred<Response>();
  let requested = false;
  const { view, props, fetch } = navigationFixture(() => {
    requested = true;
    return held.promise;
  });
  await waitFor(() => expect(requested).toBe(true));
  const signal = fetch.mock.calls.find(([path]) =>
    path.includes("/artist-identity-groups/"),
  )![1]!.signal!;
  view.rerender(
    createElement(ArtistReview, { ...props, account: "other-account" }),
  );
  expect(signal.aborted).toBe(true);
  await act(async () => held.resolve(Response.json(artistIdentityGroupDetail)));
  expect(screen.queryByRole("link", { name: "Open chosen artist" })).toBeNull();
  expect(screen.queryByText(/Saved credit for this group:/)).toBeNull();
});

test("an uncertain reset removes navigation while the exact request awaits retry", async () => {
  const { fetch } = navigationFixture(
    () => Response.json(artistIdentityGroupDetail),
    () => {
      throw new TypeError("Owned lost acknowledgement");
    },
  );
  await screen.findByRole("link", { name: "Open chosen artist" });
  fireEvent.click(
    screen.getByRole("button", { name: "Reset artist choice for this track" }),
  );
  await screen.findByRole("button", {
    name: "Retry the same artist choice request",
  });
  expect(screen.queryByRole("link", { name: "Open chosen artist" })).toBeNull();
  const writes = fetch.mock.calls.filter(
    ([, options]) => options?.method === "POST",
  );
  expect(writes).toHaveLength(1);
});
