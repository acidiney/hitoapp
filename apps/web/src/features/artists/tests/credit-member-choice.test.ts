import { createElement, type ComponentProps } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import TrackMetadataBase from "../../recordings/components/TrackMetadata";
import type { Schema } from "../../../shared/schema";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import {
  artistIdentityGroupDetail,
  unselectedArtistIdentity,
} from "../fixtures/artistIdentity";
import {
  artistAssociation,
  artistPreview,
  artistQuery,
  artistReviewAccount,
  artistReviewConnections,
  artistReviewContext,
  artistReviewDetail,
  artistReviewSettings,
  artistSearchResult,
  noAssociationChanges,
} from "../fixtures/artistReview";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
}));

function TrackMetadata(props: ComponentProps<typeof TrackMetadataBase>) {
  return createElement(
    NoticeProvider,
    null,
    createElement(ArtworkProvider, {
      account: props.account,
      children: createElement(PlaybackProvider, {
        account: props.account,
        children: createElement(TrackMetadataBase, props),
      }),
    }),
  );
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const featured = {
  artist_id: "30000000-0000-4000-8000-0000000000f1",
  name: "Featured guest",
  role: "featured",
  join: null,
} satisfies Schema<"CreditedArtistView">;
const main = {
  artist_id: "30000000-0000-4000-8000-0000000000a1",
  name: "Main voice",
  role: "main",
  join: null,
} satisfies Schema<"CreditedArtistView">;
const unlinked = {
  artist_id: null,
  name: "Nameless chorus",
  role: "featured",
  join: null,
} satisfies Schema<"CreditedArtistView">;
const detail = {
  ...artistReviewDetail,
  credited_artists: [main, featured, unlinked],
} satisfies Schema<"RecordingMetadata">;

const memberContext = {
  account_id: artistReviewContext.account_id,
  recording_id: artistReviewContext.recording_id,
  library_id: artistReviewContext.library_id,
  artist_id: featured.artist_id,
  credited_name: featured.name,
  role: featured.role,
  source_id: null,
  observation_id: artistReviewContext.observation_id,
  artist_operation_id: artistReviewContext.artist_operation_id,
  access_grant_id: artistReviewContext.access_grant_id,
  metadata_binding: artistReviewContext.metadata_binding,
  provider: { kind: "builtin_musicbrainz" },
  policy_revision: artistReviewSettings.revision,
  connection: null,
} satisfies Schema<"CreditMemberReviewContext">;
const memberAssociation = {
  ...artistAssociation,
  artist_id: featured.artist_id,
  provider: memberContext.provider,
} satisfies Schema<"ArtistAssociation">;
const memberGroup = "70000000-0000-4000-8000-0000000000f1";
const memberBase = `/api/v1/recordings/${detail.recording_id}/credit-members/${featured.artist_id}`;
const wholeBase = `/api/v1/recordings/${detail.recording_id}`;
const mainBase = `/api/v1/recordings/${detail.recording_id}/credit-members/${main.artist_id}`;
const unselectedMember = {
  recording_id: detail.recording_id,
  artist_id: featured.artist_id,
  role: featured.role,
  revision: 0,
  state: { status: "unselected" },
} satisfies Schema<"RecordingCreditMemberIdentityChoice">;

type Deferred = { promise: Promise<void>; resolve(): void };

function deferred(): Deferred {
  let resolve = () => {};
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

type ServeOptions = {
  conflict?: boolean;
  initial?: "selected";
  failLoad?: boolean;
  resetNetworkFailures?: number;
  elsewhere?: Schema<"ArtistIdentitySelection">;
  groupNames?: Record<string, string>;
  badReceipt?: "revision" | "jump" | "selection" | "association";
  losePuts?: number;
  loseResets?: number;
  conflictOnce?: boolean;
  initialReadFailures?: number;
  readbackFailures?: number;
  holdSearch?: Deferred;
  holdPreview?: Deferred;
  holdReset?: Deferred;
  holdPut?: Deferred;
};

const groupsPath = "/api/v1/artist-identity-groups/";

function serve(options: ServeOptions = {}) {
  const signals = { search: [] as AbortSignal[], preview: [] as AbortSignal[] };
  const original = {
    association_id: memberAssociation.id,
    group_id: memberGroup,
  } satisfies Schema<"ArtistIdentitySelection">;
  let revision = options.initial === "selected" ? 1 : 0;
  let selection: Schema<"ArtistIdentitySelection"> | null =
    options.initial === "selected" ? original : null;
  let readbackFailures = 0;
  let initialFailures = options.initialReadFailures ?? 0;
  let losePuts = options.losePuts ?? 0;
  let loseResets = options.loseResets ?? 0;
  let conflictOnce = options.conflictOnce ?? false;
  const receipts = new Map<
    string,
    Schema<"CreditMemberIdentityChoiceReceipt">
  >();
  const fetch = vi.fn(async (path: string, init?: RequestInit) => {
    const method = init?.method ?? "GET";
    if (
      options.failLoad &&
      method === "GET" &&
      (path === "/api/v1/metadata/settings" || path === "/api/v1/plugins")
    )
      return Response.json({ error: "down" }, { status: 502 });
    if (method === "GET" && path === "/api/v1/metadata/settings")
      return Response.json(artistReviewSettings);
    if (method === "GET" && path === "/api/v1/plugins")
      return Response.json(artistReviewConnections);
    if (method === "GET" && path === `${wholeBase}/artist-identity`)
      return Response.json(unselectedArtistIdentity);
    if (method === "GET" && path === `${wholeBase}/artist-association-changes`)
      return Response.json(noAssociationChanges);
    if (method === "GET" && path === `${mainBase}/artist-identity`)
      return Response.json({
        ...unselectedMember,
        artist_id: main.artist_id,
        role: main.role,
      });
    if (method === "GET" && path.startsWith(groupsPath)) {
      const id = path.slice(groupsPath.length).split("?")[0];
      const name = options.groupNames?.[id];
      if (name === undefined)
        return Response.json({ error: "gone" }, { status: 404 });
      return Response.json({
        ...artistIdentityGroupDetail,
        group: {
          ...artistIdentityGroupDetail.group,
          id,
          name: { ...artistIdentityGroupDetail.group.name, value: name },
        },
      });
    }
    if (method === "GET" && path === `${memberBase}/artist-identity`) {
      if (initialFailures > 0) {
        initialFailures--;
        return Response.json({ error: "read failed" }, { status: 500 });
      }
      if (readbackFailures > 0) {
        readbackFailures--;
        return Response.json({ error: "readback failed" }, { status: 500 });
      }
      const state: Schema<"RecordingArtistIdentityState"> = selection
        ? { status: "selected", ...selection }
        : { status: "unselected" };
      return Response.json({ ...unselectedMember, revision, state });
    }
    if (method === "POST" && path === `${memberBase}/artist-review/context`)
      return Response.json(memberContext);
    if (method === "POST" && path === `${memberBase}/artist-review/search`) {
      if (options.holdSearch) {
        if (init?.signal) signals.search.push(init.signal);
        await options.holdSearch.promise;
      }
      return Response.json({ ...artistSearchResult, context: memberContext });
    }
    if (method === "POST" && path === `${memberBase}/artist-review/preview`) {
      if (options.holdPreview) {
        if (init?.signal) signals.preview.push(init.signal);
        await options.holdPreview.promise;
      }
      return Response.json({ ...artistPreview, context: memberContext });
    }
    if (method === "POST" && path === `${memberBase}/artist-review/confirm`)
      return Response.json(memberAssociation);
    if (method === "POST" && path === `${memberBase}/artist-identity/reset`) {
      const body = JSON.parse(String(init?.body));
      const replay = receipts.get(body.request_id);
      if (replay) return Response.json(replay);
      if (options.holdReset) await options.holdReset.promise;
      selection = null;
      revision += 1;
      const receipt = {
        artist_id: featured.artist_id,
        recording_id: detail.recording_id,
        request_id: body.request_id,
        revision,
        role: featured.role,
        selection: null,
      } satisfies Schema<"CreditMemberIdentityChoiceReceipt">;
      receipts.set(body.request_id, receipt);
      readbackFailures = options.readbackFailures ?? 0;
      if (loseResets > 0) {
        loseResets--;
        throw new TypeError("network down");
      }
      if (init?.signal?.aborted)
        throw new DOMException("Aborted", "AbortError");
      return Response.json(receipt);
    }
    if (method === "PUT" && path === `${memberBase}/artist-identity`) {
      if (options.conflict)
        return Response.json({ error: "stale" }, { status: 409 });
      const body = JSON.parse(String(init?.body));
      const replay = receipts.get(body.request_id);
      if (replay) return Response.json(replay);
      if (options.holdPut) await options.holdPut.promise;
      if (conflictOnce) {
        conflictOnce = false;
        selection = null;
        revision = 3;
        return Response.json({ error: "stale" }, { status: 409 });
      }
      const committed = revision + 1;
      const receipt = {
        artist_id: featured.artist_id,
        recording_id: detail.recording_id,
        request_id: body.request_id,
        revision:
          options.badReceipt === "revision"
            ? 0
            : options.badReceipt === "jump"
              ? committed + 1
              : committed,
        role: featured.role,
        selection:
          options.badReceipt === "selection"
            ? null
            : options.badReceipt === "association"
              ? { ...original, association_id: elsewhere.association_id }
              : original,
      } satisfies Schema<"CreditMemberIdentityChoiceReceipt">;
      receipts.set(body.request_id, receipt);
      selection = options.elsewhere ?? original;
      revision = options.elsewhere ? committed + 1 : committed;
      readbackFailures = options.readbackFailures ?? 0;
      if (losePuts > 0) {
        losePuts--;
        throw new TypeError("network down");
      }
      if (init?.signal?.aborted)
        throw new DOMException("Aborted", "AbortError");
      return Response.json(receipt);
    }
    throw new Error(`Unexpected request ${method} ${path}`);
  });
  vi.stubGlobal("fetch", fetch);
  return Object.assign(fetch, { signals });
}

function openSection() {
  fireEvent.click(screen.getByRole("button", { name: /^Credited artists/ }));
}

function row(name: string) {
  return screen.getByRole("listitem", { name });
}

async function reachPreview() {
  fireEvent.click(
    within(row(featured.name)).getByRole("button", { name: "Choose artist" }),
  );
  const dialog = await screen.findByRole("dialog", {
    name: `Choose the artist for ${featured.name}`,
  });
  const query = await within(dialog).findByRole("searchbox", {
    name: "Search artists",
  });
  fireEvent.change(query, { target: { value: artistQuery } });
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Search artists" }),
  );
  const results = await within(dialog).findByRole("list", {
    name: "Artist search results",
  });
  fireEvent.click(
    within(within(results).getAllByRole("listitem")[1]).getByRole("button", {
      name: "Preview artist",
    }),
  );
  return dialog;
}

test("a featured member starts unchosen with a way to choose", async () => {
  serve();
  render(
    createElement(TrackMetadata, {
      account: artistReviewAccount,
      detail,
      copies: { canonical_id: detail.recording_id, copies: [] },
    }),
  );
  openSection();
  expect(
    screen
      .getByRole("button", { name: "Credited artists (2)" })
      .getAttribute("aria-expanded"),
  ).toBe("true");
  const featuredRow = row(featured.name);
  await within(featuredRow).findByText("Not chosen yet");
  expect(within(featuredRow).getByText("Featured")).toBeTruthy();
  expect(
    within(featuredRow).getByRole("button", { name: "Choose artist" }),
  ).toBeTruthy();
  expect(within(row(main.name)).getByText("Main")).toBeTruthy();
});

test("members without an artist id are not listed", async () => {
  serve();
  render(
    createElement(TrackMetadata, {
      account: artistReviewAccount,
      detail,
      copies: { canonical_id: detail.recording_id, copies: [] },
    }),
  );
  openSection();
  await within(row(featured.name)).findByText("Not chosen yet");
  expect(screen.queryByText(unlinked.name)).toBeNull();
  cleanup();
  const { container } = render(
    createElement(TrackMetadata, {
      account: artistReviewAccount,
      detail: { ...detail, credited_artists: [unlinked] },
      copies: { canonical_id: detail.recording_id, copies: [] },
    }),
  );
  expect(
    screen.queryByRole("button", { name: /^Credited artists/ }),
  ).toBeNull();
  expect(container.innerHTML).not.toContain("Credited artists");
});

test("choosing sends the exact member context to the member's own routes and shows the chosen artist", async () => {
  const fetch = serve();
  render(
    createElement(TrackMetadata, {
      account: artistReviewAccount,
      detail,
      copies: { canonical_id: detail.recording_id, copies: [] },
    }),
  );
  openSection();
  await within(row(featured.name)).findByText("Not chosen yet");
  const dialog = await reachPreview();
  fireEvent.click(
    await within(dialog).findByRole("button", { name: "Use this artist" }),
  );
  await within(row(featured.name)).findByText(
    `Chosen: ${artistPreview.profile.artist.name}`,
  );
  const writes = fetch.mock.calls.filter(
    ([, init]) => init?.method === "POST" || init?.method === "PUT",
  );
  expect(writes.map(([path, init]) => `${init?.method} ${path}`)).toEqual([
    `POST ${memberBase}/artist-review/context`,
    `POST ${memberBase}/artist-review/search`,
    `POST ${memberBase}/artist-review/preview`,
    `POST ${memberBase}/artist-review/confirm`,
    `PUT ${memberBase}/artist-identity`,
  ]);
  const put = JSON.parse(String(writes[4][1]?.body));
  expect(put).toMatchObject({
    expected_revision: 0,
    association_id: memberAssociation.id,
    context: memberContext,
  });
  expect(typeof put.request_id).toBe("string");
  expect(
    fetch.mock.calls.some(([path]) =>
      String(path).endsWith(`${detail.recording_id}/artist-identity`),
    ),
  ).toBe(false);
});

test("a changed member shows the refresh message and stays unchosen", async () => {
  serve({ conflict: true });
  render(
    createElement(TrackMetadata, {
      account: artistReviewAccount,
      detail,
      copies: { canonical_id: detail.recording_id, copies: [] },
    }),
  );
  openSection();
  await within(row(featured.name)).findByText("Not chosen yet");
  const dialog = await reachPreview();
  fireEvent.click(
    await within(dialog).findByRole("button", { name: "Use this artist" }),
  );
  await waitFor(() =>
    expect(
      within(dialog).getByText("This artist changed. Refresh and try again."),
    ).toBeTruthy(),
  );
  expect(within(row(featured.name)).queryByText(/^Chosen:/)).toBeNull();
});

test("opening the track makes no credit member request until the section is opened", async () => {
  const fetch = serve();
  render(
    createElement(TrackMetadata, {
      account: artistReviewAccount,
      detail,
      copies: { canonical_id: detail.recording_id, copies: [] },
    }),
  );
  const toggle = screen.getByRole("button", { name: "Credited artists (2)" });
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  await screen.findByRole("button", { name: "Change artist" });
  expect(
    fetch.mock.calls.filter(([path]) =>
      String(path).includes("/credit-members/"),
    ),
  ).toEqual([]);
  fireEvent.click(toggle);
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
  await within(row(featured.name)).findByText("Not chosen yet");
  expect(
    fetch.mock.calls.filter(([path]) =>
      String(path).includes("/credit-members/"),
    ).length,
  ).toBe(2);
});

test("the whole-credit artist review is unchanged beside the credited artists", async () => {
  serve();
  render(
    createElement(TrackMetadata, {
      account: artistReviewAccount,
      detail,
      copies: { canonical_id: detail.recording_id, copies: [] },
    }),
  );
  expect(screen.getByRole("heading", { name: "Linked artist" })).toBeTruthy();
  expect(
    await screen.findByRole("button", { name: "Change artist" }),
  ).toBeTruthy();
});

function renderTrack() {
  render(
    createElement(TrackMetadata, {
      account: artistReviewAccount,
      detail,
      copies: { canonical_id: detail.recording_id, copies: [] },
    }),
  );
  openSection();
}

function writeBodies(
  fetch: ReturnType<typeof serve>,
  method: string,
  target: string,
) {
  return fetch.mock.calls
    .filter(([path, init]) => init?.method === method && path === target)
    .map(([, init]) => JSON.parse(String(init?.body)));
}

function resetBodies(fetch: ReturnType<typeof serve>) {
  return writeBodies(fetch, "POST", `${memberBase}/artist-identity/reset`);
}

function isDisabled(button: HTMLElement) {
  return (button as HTMLButtonElement).disabled;
}

function loadCalls(fetch: ReturnType<typeof serve>, path: string) {
  return fetch.mock.calls.filter(
    ([called, init]) => called === path && (init?.method ?? "GET") === "GET",
  ).length;
}

async function openChooser(name: string) {
  fireEvent.click(
    within(row(name)).getByRole("button", { name: "Choose artist" }),
  );
  return screen.findByRole("dialog", {
    name: `Choose the artist for ${name}`,
  });
}

const savedName = "Saved member artist";

test("clearing a chosen member resets only that member and shows it unchosen", async () => {
  const fetch = serve({
    initial: "selected",
    groupNames: { [memberGroup]: savedName },
  });
  renderTrack();
  await within(row(featured.name)).findByText(`Chosen: ${savedName}`);
  expect(
    within(row(main.name)).queryByRole("button", { name: "Clear choice" }),
  ).toBeNull();
  fireEvent.click(
    within(row(featured.name)).getByRole("button", { name: "Clear choice" }),
  );
  await within(row(featured.name)).findByText("Not chosen yet");
  expect(resetBodies(fetch)).toEqual([
    { request_id: expect.any(String), expected_revision: 1 },
  ]);
  expect(within(row(main.name)).getByText("Not chosen yet")).toBeTruthy();
  expect(
    within(row(featured.name)).queryByRole("button", { name: "Clear choice" }),
  ).toBeNull();
  expect(
    fetch.mock.calls.some(([path]) =>
      String(path).endsWith(`${detail.recording_id}/artist-identity/reset`),
    ),
  ).toBe(false);
});

test("clearing a member works when no metadata provider can be loaded", async () => {
  const fetch = serve({
    initial: "selected",
    failLoad: true,
    groupNames: { [memberGroup]: savedName },
  });
  renderTrack();
  await within(row(featured.name)).findByText(`Chosen: ${savedName}`);
  fireEvent.click(
    within(row(featured.name)).getByRole("button", { name: "Clear choice" }),
  );
  await within(row(featured.name)).findByText("Not chosen yet");
  expect(resetBodies(fetch)).toHaveLength(1);
  expect(within(row(featured.name)).queryByRole("alert")).toBeNull();
});

test("a failed settings load stops after one request and retries only on request", async () => {
  const options: ServeOptions = {};
  const fetch = serve(options);
  renderTrack();
  await within(row(featured.name)).findByText("Not chosen yet");
  await screen.findByRole("button", { name: "Change artist" });
  options.failLoad = true;
  const settings = "/api/v1/metadata/settings";
  const plugins = "/api/v1/plugins";
  const settingsBefore = loadCalls(fetch, settings);
  const pluginsBefore = loadCalls(fetch, plugins);
  const dialog = await openChooser(featured.name);
  await within(dialog).findByRole("alert");
  await act(async () => {});
  expect(loadCalls(fetch, settings) - settingsBefore).toBe(1);
  expect(loadCalls(fetch, plugins) - pluginsBefore).toBe(1);
  expect(within(dialog).getByRole("alert")).toBeTruthy();
  fireEvent.click(within(dialog).getByRole("button", { name: "Try again" }));
  await waitFor(() =>
    expect(loadCalls(fetch, settings) - settingsBefore).toBe(2),
  );
  await act(async () => {});
  expect(loadCalls(fetch, settings) - settingsBefore).toBe(2);
  expect(loadCalls(fetch, plugins) - pluginsBefore).toBe(2);
  expect(within(dialog).getByRole("alert")).toBeTruthy();
});

const elsewhereGroup = "70000000-0000-4000-8000-0000000000e2";
const elsewhere = {
  association_id: "60000000-0000-4000-8000-0000000000e2",
  group_id: elsewhereGroup,
} satisfies Schema<"ArtistIdentitySelection">;

test("after choosing, the row names the artist the server now reports", async () => {
  serve({ elsewhere, groupNames: { [elsewhereGroup]: "Changed elsewhere" } });
  renderTrack();
  await within(row(featured.name)).findByText("Not chosen yet");
  const dialog = await reachPreview();
  fireEvent.click(
    await within(dialog).findByRole("button", { name: "Use this artist" }),
  );
  await within(row(featured.name)).findByText("Chosen: Changed elsewhere");
  expect(row(featured.name).textContent).not.toContain(
    artistPreview.profile.artist.name,
  );
});

test("after choosing, an unnamed server selection is labelled generically", async () => {
  serve({ elsewhere });
  renderTrack();
  await within(row(featured.name)).findByText("Not chosen yet");
  const dialog = await reachPreview();
  fireEvent.click(
    await within(dialog).findByRole("button", { name: "Use this artist" }),
  );
  await within(row(featured.name)).findByText("Chosen");
  expect(row(featured.name).textContent).not.toContain(
    artistPreview.profile.artist.name,
  );
});

test.each(["revision", "jump", "selection", "association"] as const)(
  "a receipt with a bad %s is not trusted and the real choice is read",
  async (badReceipt) => {
    const fetch = serve({ badReceipt });
    renderTrack();
    await within(row(featured.name)).findByText("Not chosen yet");
    const dialog = await reachPreview();
    fireEvent.click(
      await within(dialog).findByRole("button", { name: "Use this artist" }),
    );
    await within(dialog).findByRole("alert");
    await within(row(featured.name)).findByText("Chosen");
    expect(
      within(row(featured.name)).queryByText(
        `Chosen: ${artistPreview.profile.artist.name}`,
      ),
    ).toBeNull();
    expect(
      writeBodies(fetch, "PUT", `${memberBase}/artist-identity`),
    ).toHaveLength(1);
  },
);

test("cancelling the dialog stops a held search and reopening starts fresh", async () => {
  const holdSearch = deferred();
  const fetch = serve({ holdSearch });
  renderTrack();
  await within(row(featured.name)).findByText("Not chosen yet");
  const dialog = await openChooser(featured.name);
  const query = await within(dialog).findByRole("searchbox", {
    name: "Search artists",
  });
  fireEvent.change(query, { target: { value: artistQuery } });
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Search artists" }),
  );
  await waitFor(() => expect(fetch.signals.search).toHaveLength(1));
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(fetch.signals.search[0].aborted).toBe(true);
  holdSearch.resolve();
  await act(async () => {});
  const reopened = await openChooser(featured.name);
  const fresh = await within(reopened).findByRole("searchbox", {
    name: "Search artists",
  });
  expect((fresh as HTMLInputElement).value).toBe(featured.name);
  expect(
    within(reopened).queryByRole("list", { name: "Artist search results" }),
  ).toBeNull();
  expect(
    within(reopened).getByRole("button", { name: "Search artists" }),
  ).toBeTruthy();
});

test("cancelling the dialog stops a held preview and reopening starts fresh", async () => {
  const holdPreview = deferred();
  const fetch = serve({ holdPreview });
  renderTrack();
  await within(row(featured.name)).findByText("Not chosen yet");
  const dialog = await reachPreview();
  await waitFor(() => expect(fetch.signals.preview).toHaveLength(1));
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(fetch.signals.preview[0].aborted).toBe(true);
  holdPreview.resolve();
  await act(async () => {});
  const reopened = await openChooser(featured.name);
  await within(reopened).findByRole("searchbox", { name: "Search artists" });
  expect(
    within(reopened).queryByRole("list", { name: "Artist search results" }),
  ).toBeNull();
  expect(
    within(reopened).queryByRole("region", { name: "Chosen candidate" }),
  ).toBeNull();
});

test("cancelling after an accepted choice keeps it and re-reads the real choice", async () => {
  const fetch = serve({ readbackFailures: 1 });
  renderTrack();
  await within(row(featured.name)).findByText("Not chosen yet");
  const dialog = await reachPreview();
  fireEvent.click(
    await within(dialog).findByRole("button", { name: "Use this artist" }),
  );
  await within(dialog).findByRole("alert");
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await within(row(featured.name)).findByText(
    `Chosen: ${artistPreview.profile.artist.name}`,
  );
  expect(
    fetch.mock.calls.filter(([, init]) => init?.method === "PUT"),
  ).toHaveLength(1);
});

const memberIdentity = `${memberBase}/artist-identity`;
const unresolvedNote = "Hito could not confirm the last change to this credit.";

async function chooseAndLose(options: ServeOptions) {
  const fetch = serve(options);
  renderTrack();
  await within(row(featured.name)).findByText("Not chosen yet");
  const dialog = await reachPreview();
  fireEvent.click(
    await within(dialog).findByRole("button", { name: "Use this artist" }),
  );
  await within(dialog).findByRole("alert");
  return { fetch, dialog };
}

test("a choice whose response was lost is replayed with the same request", async () => {
  const { fetch, dialog } = await chooseAndLose({ losePuts: 1 });
  expect(within(row(featured.name)).getByText(unresolvedNote)).toBeTruthy();
  expect(
    isDisabled(within(dialog).getByRole("button", { name: "Use this artist" })),
  ).toBe(true);
  fireEvent.click(within(dialog).getByRole("button", { name: "Check again" }));
  await within(row(featured.name)).findByText(
    `Chosen: ${artistPreview.profile.artist.name}`,
  );
  const puts = writeBodies(fetch, "PUT", memberIdentity);
  expect(puts).toHaveLength(2);
  expect(puts[1]).toEqual(puts[0]);
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

test("a lost choice response can be checked again from the row after cancelling", async () => {
  const { fetch, dialog } = await chooseAndLose({ losePuts: 1 });
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(
    isDisabled(
      within(row(featured.name)).getByRole("button", { name: "Choose artist" }),
    ),
  ).toBe(true);
  fireEvent.click(
    within(row(featured.name)).getByRole("button", { name: "Check again" }),
  );
  await within(row(featured.name)).findByText(
    `Chosen: ${artistPreview.profile.artist.name}`,
  );
  const puts = writeBodies(fetch, "PUT", memberIdentity);
  expect(puts).toHaveLength(2);
  expect(puts[1].request_id).toBe(puts[0].request_id);
});

test("an accepted choice with a failed readback blocks new writes until checked", async () => {
  const { fetch, dialog } = await chooseAndLose({ readbackFailures: 1 });
  expect(
    isDisabled(within(dialog).getByRole("button", { name: "Use this artist" })),
  ).toBe(true);
  fireEvent.click(within(dialog).getByRole("button", { name: "Check again" }));
  await within(row(featured.name)).findByText(
    `Chosen: ${artistPreview.profile.artist.name}`,
  );
  expect(writeBodies(fetch, "PUT", memberIdentity)).toHaveLength(1);
});

test("a rejected choice is dropped, re-read, and a new attempt uses the new revision", async () => {
  const fetch = serve({ conflictOnce: true });
  renderTrack();
  await within(row(featured.name)).findByText("Not chosen yet");
  const dialog = await reachPreview();
  fireEvent.click(
    await within(dialog).findByRole("button", { name: "Use this artist" }),
  );
  await within(dialog).findByText(
    "This artist changed. Refresh and try again.",
  );
  await waitFor(() =>
    expect(
      isDisabled(
        within(dialog).getByRole("button", { name: "Use this artist" }),
      ),
    ).toBe(false),
  );
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Use this artist" }),
  );
  await within(row(featured.name)).findByText(
    `Chosen: ${artistPreview.profile.artist.name}`,
  );
  const puts = writeBodies(fetch, "PUT", memberIdentity);
  expect(puts).toHaveLength(2);
  expect(puts[0].expected_revision).toBe(0);
  expect(puts[1].expected_revision).toBe(3);
  expect(puts[1].request_id).not.toBe(puts[0].request_id);
});

async function clearAndLose(options: ServeOptions) {
  const fetch = serve({
    initial: "selected",
    groupNames: { [memberGroup]: savedName },
    ...options,
  });
  renderTrack();
  await within(row(featured.name)).findByText(`Chosen: ${savedName}`);
  fireEvent.click(
    within(row(featured.name)).getByRole("button", { name: "Clear choice" }),
  );
  await within(row(featured.name)).findByRole("alert");
  return fetch;
}

test("a reset whose response was lost is replayed with the same request", async () => {
  const fetch = await clearAndLose({ loseResets: 1 });
  expect(
    isDisabled(
      within(row(featured.name)).getByRole("button", { name: "Clear choice" }),
    ),
  ).toBe(true);
  fireEvent.click(
    within(row(featured.name)).getByRole("button", { name: "Check again" }),
  );
  await within(row(featured.name)).findByText("Not chosen yet");
  const bodies = resetBodies(fetch);
  expect(bodies).toHaveLength(2);
  expect(bodies[1]).toEqual(bodies[0]);
  expect(bodies[0].expected_revision).toBe(1);
});

test("a reset receipt with a failed readback stays unresolved until read", async () => {
  const fetch = await clearAndLose({ readbackFailures: 1 });
  expect(within(row(featured.name)).getByText(/^Chosen/)).toBeTruthy();
  fireEvent.click(
    within(row(featured.name)).getByRole("button", { name: "Check again" }),
  );
  await within(row(featured.name)).findByText("Not chosen yet");
  expect(resetBodies(fetch)).toHaveLength(1);
  expect(
    within(row(featured.name)).queryByRole("button", { name: "Check again" }),
  ).toBeNull();
});

test("the chooser is blocked while a reset is unresolved", async () => {
  const fetch = await clearAndLose({ loseResets: 1 });
  const choose = within(row(featured.name)).getByRole("button", {
    name: "Choose artist",
  });
  expect(isDisabled(choose)).toBe(true);
  fireEvent.click(choose);
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(writeBodies(fetch, "PUT", memberIdentity)).toHaveLength(0);
  fireEvent.click(
    within(row(featured.name)).getByRole("button", { name: "Check again" }),
  );
  await within(row(featured.name)).findByText("Not chosen yet");
});

test("a failed first read shows the failure and can be tried again", async () => {
  serve({ initialReadFailures: 1 });
  renderTrack();
  await within(row(featured.name)).findByText("Could not load");
  expect(within(row(featured.name)).getByRole("alert")).toBeTruthy();
  expect(
    isDisabled(
      within(row(featured.name)).getByRole("button", { name: "Choose artist" }),
    ),
  ).toBe(true);
  fireEvent.click(
    within(row(featured.name)).getByRole("button", { name: "Try again" }),
  );
  await within(row(featured.name)).findByText("Not chosen yet");
  const dialog = await reachPreview();
  expect(
    await within(dialog).findByRole("button", { name: "Use this artist" }),
  ).toBeTruthy();
});

function toggleSection() {
  const toggle = screen.getByRole("button", { name: /^Credited artists/ });
  fireEvent.click(toggle);
  fireEvent.click(toggle);
}

async function holdClear(holdReset: Deferred) {
  const fetch = serve({
    initial: "selected",
    groupNames: { [memberGroup]: savedName },
    holdReset,
  });
  renderTrack();
  await within(row(featured.name)).findByText(`Chosen: ${savedName}`);
  fireEvent.click(
    within(row(featured.name)).getByRole("button", { name: "Clear choice" }),
  );
  await waitFor(() => expect(resetBodies(fetch)).toHaveLength(1));
  return fetch;
}

async function holdChoose(holdPut: Deferred) {
  const fetch = serve({ holdPut });
  renderTrack();
  await within(row(featured.name)).findByText("Not chosen yet");
  const dialog = await reachPreview();
  fireEvent.click(
    await within(dialog).findByRole("button", { name: "Use this artist" }),
  );
  await waitFor(() =>
    expect(writeBodies(fetch, "PUT", memberIdentity)).toHaveLength(1),
  );
  return fetch;
}

function expectBlockedRow() {
  const current = row(featured.name);
  expect(within(current).getByText(unresolvedNote)).toBeTruthy();
  expect(
    isDisabled(within(current).getByRole("button", { name: "Choose artist" })),
  ).toBe(true);
}

test("collapsing the section keeps an in-flight reset unresolved", async () => {
  const holdReset = deferred();
  const fetch = await holdClear(holdReset);
  toggleSection();
  expectBlockedRow();
  expect(
    isDisabled(
      within(row(featured.name)).getByRole("button", { name: "Clear choice" }),
    ),
  ).toBe(true);
  holdReset.resolve();
  await within(row(featured.name)).findByText("Not chosen yet");
  expect(resetBodies(fetch)).toHaveLength(1);
});

test("collapsing the section keeps an in-flight choice unresolved", async () => {
  const holdPut = deferred();
  const fetch = await holdChoose(holdPut);
  toggleSection();
  const dialog = screen.getByRole("dialog", {
    name: `Choose the artist for ${featured.name}`,
  });
  expectBlockedRow();
  expect(
    isDisabled(within(dialog).getByRole("button", { name: "Saving artist…" })),
  ).toBe(true);
  holdPut.resolve();
  await within(row(featured.name)).findByText(
    `Chosen: ${artistPreview.profile.artist.name}`,
  );
  expect(writeBodies(fetch, "PUT", memberIdentity)).toHaveLength(1);
});

test("a remounted row adopts a reset the server may still commit", async () => {
  const holdReset = deferred();
  const fetch = await holdClear(holdReset);
  cleanup();
  renderTrack();
  await within(row(featured.name)).findByText(`Chosen: ${savedName}`);
  expectBlockedRow();
  expect(
    isDisabled(
      within(row(featured.name)).getByRole("button", { name: "Clear choice" }),
    ),
  ).toBe(true);
  holdReset.resolve();
  await act(async () => {});
  const check = within(row(featured.name)).getByRole("button", {
    name: "Check again",
  });
  await waitFor(() => expect(isDisabled(check)).toBe(false));
  fireEvent.click(check);
  await within(row(featured.name)).findByText("Not chosen yet");
  const bodies = resetBodies(fetch);
  expect(bodies).toHaveLength(2);
  expect(bodies[1]).toEqual(bodies[0]);
});

test("a remounted row adopts a choice the server may still commit", async () => {
  const holdPut = deferred();
  const fetch = await holdChoose(holdPut);
  cleanup();
  renderTrack();
  await within(row(featured.name)).findByText("Not chosen yet");
  expectBlockedRow();
  holdPut.resolve();
  await act(async () => {});
  const check = within(row(featured.name)).getByRole("button", {
    name: "Check again",
  });
  await waitFor(() => expect(isDisabled(check)).toBe(false));
  fireEvent.click(check);
  await within(row(featured.name)).findByText(
    `Chosen: ${artistPreview.profile.artist.name}`,
  );
  const puts = writeBodies(fetch, "PUT", memberIdentity);
  expect(puts).toHaveLength(2);
  expect(puts[1]).toEqual(puts[0]);
});
