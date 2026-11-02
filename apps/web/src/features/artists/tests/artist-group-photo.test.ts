import { createElement, type AnchorHTMLAttributes } from "react";
import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistIndexPage from "../pages/ArtistIndexPage";
import ArtistIdentityGroupPage from "../pages/ArtistIdentityGroupPage";
import { artistIdentityBrowsing } from "../fixtures/artistIdentityBrowsing";

const MUSICBRAINZ = "0f800000-0000-4000-8000-0000000000a1";
const CREDIT = "0123456789abcdef0123456789abcdef";

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
  router: { get: vi.fn() },
  usePage: () => ({ url: window.location.pathname }),
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

afterEach(cleanup);

type Variant = {
  provider: { kind: "builtin_musicbrainz" } | { kind: "http_plugin" };
  provider_artist_id: string;
  credited_photo_artist_id?: string;
};

function pages(variant: Variant) {
  const { index, detail, group } = artistIdentityBrowsing();
  const changed = { ...group, ...variant } as typeof group;
  return {
    index: {
      ...index,
      identity_groups: { ...index.identity_groups, items: [changed] },
    },
    detail: { ...detail, detail: { ...detail.detail, group: changed } },
  };
}

function sources() {
  return Array.from(document.querySelectorAll("img"))
    .map((image) => image.getAttribute("src"))
    .filter((src) => src?.startsWith("/api/v1/cover-art/"));
}

const musicbrainz: Variant = {
  provider: { kind: "builtin_musicbrainz" },
  provider_artist_id: MUSICBRAINZ,
};

test("a chosen-artist card and page ask the musicbrainz photo while no plugin credit is offered", () => {
  const { index, detail } = pages(musicbrainz);
  render(createElement(ArtistIndexPage, index as never));
  expect(sources()).toContain(`/api/v1/cover-art/artist/${MUSICBRAINZ}`);
  cleanup();
  render(createElement(ArtistIdentityGroupPage, detail as never));
  expect(sources()).toContain(`/api/v1/cover-art/artist/${MUSICBRAINZ}`);
});

test("an offered member credit makes the card and page ask the credited artist route instead", () => {
  const { index, detail } = pages({
    ...musicbrainz,
    credited_photo_artist_id: CREDIT,
  });
  render(createElement(ArtistIndexPage, index as never));
  expect(sources()).toContain(`/api/v1/cover-art/credited-artist/${CREDIT}`);
  expect(sources().some((src) => src?.includes("/cover-art/artist/"))).toBe(
    false,
  );
  cleanup();
  render(createElement(ArtistIdentityGroupPage, detail as never));
  expect(sources()).toContain(`/api/v1/cover-art/credited-artist/${CREDIT}`);
  expect(sources().some((src) => src?.includes("/cover-art/artist/"))).toBe(
    false,
  );
});

test("a group without a musicbrainz identity asks only the offered member credit", () => {
  const plain: Variant = {
    provider: { kind: "http_plugin" },
    provider_artist_id: "plugin-artist",
  };
  const without = pages(plain);
  render(createElement(ArtistIndexPage, without.index as never));
  expect(sources()).toEqual([]);
  cleanup();
  const offered = pages({ ...plain, credited_photo_artist_id: CREDIT });
  render(createElement(ArtistIndexPage, offered.index as never));
  expect(sources()).toEqual([`/api/v1/cover-art/credited-artist/${CREDIT}`]);
  cleanup();
  render(createElement(ArtistIdentityGroupPage, offered.detail as never));
  expect(sources()).toContain(`/api/v1/cover-art/credited-artist/${CREDIT}`);
});
