import { createElement, type ComponentProps } from "react";
import {
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
import { unselectedArtistIdentity } from "../fixtures/artistIdentity";
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

function serve(options: { conflict?: boolean } = {}) {
  let chosen = false;
  const fetch = vi.fn(async (path: string, init?: RequestInit) => {
    const method = init?.method ?? "GET";
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
    if (method === "GET" && path === `${memberBase}/artist-identity`)
      return Response.json(
        chosen
          ? {
              ...unselectedMember,
              revision: 1,
              state: {
                status: "selected",
                group_id: memberGroup,
                association_id: memberAssociation.id,
              },
            }
          : unselectedMember,
      );
    if (method === "POST" && path === `${memberBase}/artist-review/context`)
      return Response.json(memberContext);
    if (method === "POST" && path === `${memberBase}/artist-review/search`)
      return Response.json({ ...artistSearchResult, context: memberContext });
    if (method === "POST" && path === `${memberBase}/artist-review/preview`)
      return Response.json({ ...artistPreview, context: memberContext });
    if (method === "POST" && path === `${memberBase}/artist-review/confirm`)
      return Response.json(memberAssociation);
    if (method === "PUT" && path === `${memberBase}/artist-identity`) {
      if (options.conflict)
        return Response.json({ error: "stale" }, { status: 409 });
      chosen = true;
      const body = JSON.parse(String(init?.body));
      return Response.json({
        artist_id: featured.artist_id,
        recording_id: detail.recording_id,
        request_id: body.request_id,
        revision: 1,
        role: featured.role,
        selection: {
          association_id: memberAssociation.id,
          group_id: memberGroup,
        },
      } satisfies Schema<"CreditMemberIdentityChoiceReceipt">);
    }
    throw new Error(`Unexpected request ${method} ${path}`);
  });
  vi.stubGlobal("fetch", fetch);
  return fetch;
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
