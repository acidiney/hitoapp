import { createElement, type AnchorHTMLAttributes } from "react";
import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistIdentityGroupPage from "../pages/ArtistIdentityGroupPage";
import { artistIdentityBrowsing } from "../fixtures/artistIdentityBrowsing";

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

test("the artist header names the artist and shows the artist cover", () => {
  const { detail, group } = artistIdentityBrowsing();
  const { container } = render(createElement(ArtistIdentityGroupPage, detail));
  const header = container.querySelector<HTMLElement>(".artist-header")!;
  expect(header.querySelector("h1")!.textContent).toBe(group.name.value);
  expect(header.querySelector(".artist-header-photo .artwork")).toBeTruthy();
});
