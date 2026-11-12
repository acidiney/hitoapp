import { createElement, useState, type AnchorHTMLAttributes } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import ArtistIndexPage from "../pages/ArtistIndexPage";
import { artistIdentityBrowsing } from "../fixtures/artistIdentityBrowsing";

const navigation = vi.hoisted(() => ({ get: vi.fn() }));
const memory = vi.hoisted(() => new Map<string, unknown>());
vi.mock("@inertiajs/react", () => ({
  router: navigation,
  usePage: () => ({ url: window.location.pathname + window.location.search }),
  useRemember: (initial: unknown, key: string) => {
    const [value, setValue] = useState(memory.get(key) ?? initial);
    return [
      value,
      (next: unknown) =>
        setValue((previous: unknown) => {
          const resolved =
            typeof next === "function"
              ? (next as (value: unknown) => unknown)(previous)
              : next;
          memory.set(key, resolved);
          return resolved;
        }),
    ];
  },
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

const watched: { element: Element; fire: () => void }[] = [];

beforeEach(() => {
  memory.clear();
  watched.length = 0;
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      constructor(private callback: IntersectionObserverCallback) {}
      observed: Element[] = [];
      observe(element: Element) {
        this.observed.push(element);
        watched.push({
          element,
          fire: () =>
            this.callback(
              [{ isIntersecting: true } as IntersectionObserverEntry],
              this as unknown as IntersectionObserver,
            ),
        });
      }
      disconnect() {
        for (const element of this.observed) {
          const at = watched.findIndex((entry) => entry.element === element);
          if (at >= 0) watched.splice(at, 1);
        }
      }
    },
  );
});
afterEach(() => {
  cleanup();
  window.history.replaceState(null, "", "/");
});

function credit(id: string) {
  return {
    id,
    library_id: "owned-local-library",
    name: `Credit ${id}`,
    recording_count: 1,
    release_count: 0,
    artwork_recording_id: null,
  };
}

function setup() {
  const { index } = artistIdentityBrowsing();
  window.history.replaceState(
    null,
    "",
    "/artists?q=Saved&library=owned-local-library&limit=2",
  );
  const requests: string[] = [];
  const fetch = vi.fn(async (path: string) => {
    requests.push(path);
    if (path.startsWith("/api/v1/artist-matching"))
      return Response.json({ items: [], total: 0 });
    if (path.startsWith("/api/v1/artists?"))
      return Response.json({
        items: [credit("third")],
        total: 3,
        limit: 2,
        offset: 2,
      });
    if (path.startsWith("/api/v1/artist-identity-groups?"))
      return Response.json({
        items: [{ ...index.identity_groups.items[0], id: "third-group" }],
        total: 3,
        limit: 2,
        offset: 2,
      });
    return Response.json({ error: "Unexpected" }, { status: 500 });
  });
  vi.stubGlobal("fetch", fetch);
  render(createElement(ArtistIndexPage, index));
  const credits = screen.getByRole("region", {
    name: "Artists in your library",
  });
  const chosen = screen.getByRole("region", { name: "Chosen artists" });
  return { index, requests, credits, chosen };
}

function reach(region: HTMLElement) {
  const entry = watched.find(({ element }) => region.contains(element));
  if (!entry) throw new Error("This list has no scroll sentinel");
  act(() => entry.fire());
}

test("the artist grids no longer page with Previous and Next", () => {
  const { credits, chosen } = setup();
  for (const region of [credits, chosen]) {
    expect(within(region).queryByRole("button", { name: "Next" })).toBeNull();
    expect(
      within(region).queryByRole("button", { name: "Previous" }),
    ).toBeNull();
    expect(
      within(region).getByRole("button", { name: "Load more" }),
    ).toBeTruthy();
  }
  expect(screen.getByText("Showing 2 of 3 artists")).toBeTruthy();
});

test("reaching the end of the library credits appends the next page to the same grid", async () => {
  const { index, credits, requests } = setup();
  reach(credits);
  await within(credits).findByRole("link", { name: /Credit third/ });
  expect(within(credits).getAllByRole("link")).toHaveLength(3);
  const url = new URL(
    requests.find((path) => path.startsWith("/api/v1/artists?"))!,
    "http://hito.test",
  );
  expect(Object.fromEntries(url.searchParams)).toEqual({
    q: index.query,
    library: "owned-local-library",
    limit: "2",
    offset: "2",
    sort: "recent",
  });
  expect(navigation.get).not.toHaveBeenCalled();
  expect(
    within(credits).queryByRole("button", { name: "Load more" }),
  ).toBeNull();
});

test("the Load more button loads the next chosen artists without a scroll signal", async () => {
  const { chosen, requests } = setup();
  fireEvent.click(within(chosen).getByRole("button", { name: "Load more" }));
  await waitFor(() =>
    expect(within(chosen).getAllByRole("link")).toHaveLength(3),
  );
  expect(
    requests.some((path) => path.startsWith("/api/v1/artist-identity-groups?")),
  ).toBe(true);
});

test("a failed load keeps the loaded artists, says so, and retries on request", async () => {
  const { credits } = setup();
  const fetch = vi.fn(async () =>
    Response.json({ error: "Down" }, { status: 500 }),
  );
  vi.stubGlobal("fetch", fetch);
  fireEvent.click(within(credits).getByRole("button", { name: "Load more" }));
  expect(await within(credits).findByRole("alert")).toBeTruthy();
  expect(within(credits).getAllByRole("link")).toHaveLength(2);
  expect(watched.some(({ element }) => credits.contains(element))).toBe(false);
  expect(fetch).toHaveBeenCalledTimes(1);
  fireEvent.click(within(credits).getByRole("button", { name: "Try again" }));
  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
});

test("coming back to the artists keeps every page already loaded without asking again", async () => {
  const first = setup();
  fireEvent.click(
    within(first.credits).getByRole("button", { name: "Load more" }),
  );
  await within(first.credits).findByRole("link", { name: /Credit third/ });
  cleanup();
  const fetch = vi.fn();
  vi.stubGlobal("fetch", fetch);
  render(createElement(ArtistIndexPage, first.index));
  const credits = screen.getByRole("region", {
    name: "Artists in your library",
  });
  expect(within(credits).getAllByRole("link")).toHaveLength(3);
  expect(
    fetch.mock.calls.filter(([path]) => String(path).includes("offset=")),
  ).toEqual([]);
});

test("the artists page sorts its library credits by name when asked and leaves chosen artists in order", async () => {
  window.history.replaceState(
    null,
    "",
    "/artists?sort=title&limit=2&library=owned-local-library",
  );
  const { index, requests, credits, chosen } = setup();
  window.history.replaceState(
    null,
    "",
    "/artists?sort=title&limit=2&library=owned-local-library",
  );
  cleanup();
  render(createElement(ArtistIndexPage, index));
  expect(screen.getByRole("combobox", { name: "Sort" }).textContent).toBe(
    "SortName",
  );
  requests.length = 0;
  const region = screen.getByRole("region", {
    name: "Artists in your library",
  });
  fireEvent.click(within(region).getByRole("button", { name: "Load more" }));
  await within(region).findByRole("link", { name: /Credit third/ });
  fireEvent.click(
    within(screen.getByRole("region", { name: "Chosen artists" })).getByRole(
      "button",
      { name: "Load more" },
    ),
  );
  await waitFor(() =>
    expect(
      requests.some((path) =>
        path.startsWith("/api/v1/artist-identity-groups?"),
      ),
    ).toBe(true),
  );
  expect(
    requests.find((path) => path.startsWith("/api/v1/artists?")),
  ).toContain("sort=title");
  expect(
    requests.find((path) => path.startsWith("/api/v1/artist-identity-groups?")),
  ).not.toContain("sort=");
  expect(credits).toBeTruthy();
  expect(chosen).toBeTruthy();
});
