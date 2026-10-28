import { createElement, type AnchorHTMLAttributes } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import ArtistIdentityGroupPage from "../pages/ArtistIdentityGroupPage";
import type { components } from "../../../generated";
import { artistIdentityBrowsing } from "../fixtures/artistIdentityBrowsing";

type Schema = components["schemas"];

const navigation = vi.hoisted(() => ({ reload: vi.fn(), get: vi.fn() }));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({
    playQueue: vi.fn(),
    subscribe: () => () => {},
    snapshot: () => ({ current: null }),
    subscribePosition: vi.fn(),
  }),
}));
vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn(), pending: false }),
}));
vi.mock("@inertiajs/react", () => ({
  router: navigation,
  usePage: () => ({ url: window.location.pathname + window.location.search }),
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

beforeEach(() => navigation.reload.mockClear());
afterEach(() => cleanup());

function page(admin: boolean) {
  const { detail, group } = artistIdentityBrowsing();
  return {
    props: { ...detail, user: { ...detail.user, admin } },
    group,
    other: {
      ...group,
      id: "other-chosen-group",
      provider_artist_id: "separate-provider-artist",
      name: { ...group.name, value: "Other chosen artist" },
    },
  };
}

function owned(
  admin: boolean,
  options: { shareable?: boolean; lostFirst?: boolean } = {},
) {
  const { props, group, other } = page(admin);
  let revision = 0;
  let latest: Schema["ArtistIdentityChangeSummary"] | null = null;
  let corrections = 0;
  let lost = options.lostFirst ?? false;
  const bodies: { path: string; body: Record<string, unknown> }[] = [];
  const fetch = vi.fn(async (path: string, init?: RequestInit) => {
    const method = init?.method ?? "GET";
    const base = `/api/v1/artist-identity-groups/${group.id}`;
    if (method === "GET" && path === `${base}/changes`)
      return Response.json({
        group_id: group.id,
        revision,
        latest,
      } satisfies Schema["ArtistIdentityGroupChanges"]);
    if (method === "GET" && path === "/api/v1/artist-identity-groups?limit=50")
      return Response.json({
        items: [group, other],
        total: 2,
        limit: 50,
        offset: 0,
      } satisfies Schema["ArtistIdentityGroupPage"]);
    if (method === "GET" && path === "/api/v1/artist-corrections") {
      expect(admin).toBe(true);
      return Response.json({
        revision: corrections,
        items: [],
      } satisfies Schema["SharedArtistCorrections"]);
    }
    if (method === "POST") {
      const body = JSON.parse(init?.body as string);
      bodies.push({ path, body });
      if (lost) {
        lost = false;
        throw new TypeError("network lost");
      }
      if (path === `${base}/split`) {
        expect(body.expected_revision).toBe(revision);
        revision += 1;
        latest = {
          revision,
          kind: "split",
          recordings: body.recordings.length || 1,
          retired: 0,
          destination_group_id: other.id,
          undoable: true,
          shareable: options.shareable ?? false,
        };
        return Response.json({
          request_id: body.request_id,
          group_id: group.id,
          revision,
          kind: "split",
          recordings: latest.recordings,
          destination_group_id: other.id,
        } satisfies Schema["ArtistIdentityChangeReceipt"]);
      }
      if (path === `${base}/undo`) {
        expect(body.expected_revision).toBe(revision);
        revision += 1;
        latest = { ...latest!, revision, kind: "undo", shareable: false };
        return Response.json({
          request_id: body.request_id,
          group_id: group.id,
          revision,
          kind: "undo",
          recordings: 1,
          destination_group_id: null,
        } satisfies Schema["ArtistIdentityChangeReceipt"]);
      }
      if (path === "/api/v1/artist-corrections/promote") {
        corrections += 1;
        latest = {
          ...latest!,
          shareable: false,
          shared: { id: "shared-correction", revision: 1 },
        };
        return Response.json({
          request_id: body.request_id,
          correction_id: "shared-correction",
          revision: 1,
          applied: true,
          recordings: 1,
        } satisfies Schema["SharedArtistCorrectionReceipt"]);
      }
      if (path === "/api/v1/artist-corrections/shared-correction/undo") {
        latest = { ...latest!, shared: null };
        return Response.json({
          request_id: body.request_id,
          correction_id: "shared-correction",
          revision: 2,
          applied: false,
          recordings: 1,
        } satisfies Schema["SharedArtistCorrectionReceipt"]);
      }
    }
    throw new Error(`Unexpected request ${method} ${path}`);
  });
  vi.stubGlobal("fetch", fetch);
  return { props, group, other, fetch, bodies };
}

async function open(props: ReturnType<typeof page>["props"]) {
  render(createElement(ArtistIdentityGroupPage, props));
  fireEvent.click(screen.getByRole("button", { name: "Correct this artist" }));
  await screen.findByRole("button", { name: "Move selected tracks" });
  await waitFor(() =>
    expect(
      (screen.getByRole("combobox", { name: "Move to" }) as HTMLSelectElement).options.length,
    ).toBe(2),
  );
}

test("the panel stays closed without requests, then moves chosen tracks to another artist and undoes it", async () => {
  const { props, other, fetch, bodies } = owned(false);
  render(createElement(ArtistIdentityGroupPage, props));
  expect(fetch).not.toHaveBeenCalled();
  cleanup();
  await open(props);
  const move = screen.getByRole("button", {
    name: "Move selected tracks",
  }) as HTMLButtonElement;
  expect(move.disabled).toBe(true);
  expect(screen.queryByRole("button", { name: /for everyone/ })).toBeNull();
  fireEvent.click(
    screen.getByRole("checkbox", {
      name: `Move ${props.detail.recordings.items[1].recording.title}`,
    }),
  );
  fireEvent.change(screen.getByRole("combobox", { name: "Move to" }), {
    target: { value: other.id },
  });
  fireEvent.click(move);
  await screen.findByRole("status");
  expect(bodies[0].path).toBe(
    `/api/v1/artist-identity-groups/${props.detail.group.id}/split`,
  );
  expect(bodies[0].body).toEqual({
    request_id: expect.any(String),
    expected_revision: 0,
    recordings: [props.detail.recordings.items[1].recording.id],
    releases: [],
    destination: {
      kind: "identity",
      provider: other.provider,
      provider_artist_id: other.provider_artist_id,
    },
  } satisfies Schema["SplitArtistIdentityGroup"] & Record<string, unknown>);
  expect(navigation.reload).toHaveBeenCalledTimes(1);
  fireEvent.click(
    await screen.findByRole("button", { name: "Undo last artist change" }),
  );
  await waitFor(() => expect(bodies).toHaveLength(2));
  expect(bodies[1].body).toEqual({
    request_id: expect.any(String),
    expected_revision: 1,
  } satisfies Schema["UndoArtistIdentityChange"]);
  expect(bodies[1].body.request_id).not.toBe(bodies[0].body.request_id);
  expect(
    fetch.mock.calls.filter(([path]) => path.includes("/artist-corrections")),
  ).toHaveLength(0);
});

test("selecting an album sends the release and a lost acknowledgement is retried with the same request", async () => {
  const { props, bodies } = owned(false, { lostFirst: true });
  await open(props);
  fireEvent.click(
    screen.getByRole("checkbox", {
      name: `Move tracks on ${props.detail.releases.items[0].title}`,
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Move selected tracks" }));
  const retry = await screen.findByRole("button", {
    name: "Retry the same artist correction request",
  });
  expect(screen.getByRole("alert")).toBeTruthy();
  fireEvent.click(retry);
  await screen.findByRole("status");
  expect(bodies).toHaveLength(2);
  expect(bodies[1]).toEqual(bodies[0]);
  expect(bodies[0].body).toMatchObject({
    recordings: [],
    releases: [props.detail.releases.items[0].id],
    destination: { kind: "credit" },
  });
});

test("only an administrator can share a split and withdraw the shared correction", async () => {
  const { props, group, bodies } = owned(true, { shareable: true });
  await open(props);
  fireEvent.click(
    screen.getByRole("checkbox", {
      name: `Move ${props.detail.recordings.items[0].recording.title}`,
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Move selected tracks" }));
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Apply this correction for everyone",
    }),
  );
  await screen.findByRole("button", { name: "Withdraw the shared correction" });
  expect(bodies[1]).toEqual({
    path: "/api/v1/artist-corrections/promote",
    body: {
      request_id: expect.any(String),
      expected_revision: 0,
      source: { kind: "split", group_id: group.id, revision: 1 },
    },
  });
  fireEvent.click(
    screen.getByRole("button", { name: "Withdraw the shared correction" }),
  );
  await waitFor(() => expect(bodies).toHaveLength(3));
  expect(bodies[2].path).toBe("/api/v1/artist-corrections/shared-correction/undo");
  expect(bodies[2].body).toEqual({
    request_id: expect.any(String),
    expected_revision: 1,
  });
});
