import { createElement } from "react";
import { fireEvent, render } from "@testing-library/react";
import { expect, test } from "vitest";
import CatalogCover from "../components/CatalogCover";

const group = "6c1e3a2e-6b0d-4a52-8d58-0b6a5d6f7a10";
const edition = "9a4d1d3c-1f0e-4f3a-9a77-2a1a3c5e8b21";

function cover(groupId: string, editionId: string | null) {
  return createElement(CatalogCover, {
    group: groupId,
    edition: editionId,
    title: "Album",
    artist: "Artist",
  });
}

test("a MusicBrainz release cover is tried first and the release group cover second, then the fallback cover", () => {
  const view = render(cover(group, edition));
  const image = () => view.container.querySelector("img");
  expect(image()?.getAttribute("src")).toBe(
    `/api/v1/cover-art/release/${edition}`,
  );
  fireEvent.error(image()!);
  expect(image()?.getAttribute("src")).toBe(
    `/api/v1/cover-art/release-group/${group}`,
  );
  fireEvent.error(image()!);
  expect(image()).toBeNull();
  expect(
    view.getByRole("img", { hidden: true, name: "Cover of Album" }),
  ).toBeTruthy();
});

test("identifiers that are not MusicBrainz identifiers request no cover and show the fallback cover", () => {
  const view = render(cover("opaque:release-group/one", "opaque:edition/one"));
  expect(view.container.querySelector("img")).toBeNull();
  expect(
    view.getByRole("img", { hidden: true, name: "Cover of Album" }),
  ).toBeTruthy();
});

test("a loaded cover hides the fallback cover", () => {
  const view = render(cover(group, null));
  fireEvent.load(view.container.querySelector("img")!);
  expect(view.container.querySelector("[data-loaded]")).not.toBeNull();
  expect(
    view.queryByRole("img", { hidden: true, name: "Cover of Album" }),
  ).toBeNull();
});
