import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import DiscoverPage from "../pages/DiscoverPage";
import { skippingFeatured } from "../fixtures/skippingFeatured";

const { player } = vi.hoisted(() => ({
  player: { prepareSelection: vi.fn(() => vi.fn().mockResolvedValue(true)) },
}));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("@inertiajs/react", () => ({
  Link: ({ href, children, ...rest }: { href: string; children: ReactNode }) =>
    createElement("a", { href, ...rest }, children),
}));
const connection: Schema<"PluginInstallation"> = {
  id: "connection",
  service_id: "service",
  revision: 1,
  checked_at: 1,
  credential_set: false,
  manifest: {
    id: "org.hito.fixture",
    name: "Fixture service",
    protocol_version: 1,
    capabilities: ["search", "sources"],
  },
};
const props: Schema<"DiscoverPageProps"> = {
  errors: {},
  native_playback: false,
  lock: { pin_enabled: false, idle_minutes: 0, locked: false },
  user: { id: "alice", username: "Alice", admin: false },
  libraries: [],
  playlists: [],
  connections: [connection],
  service_names: { service: "Home collection" },
  genres: [],
  artists: { items: [], total: 0, offset: 0, limit: 6 },
};

test("discovery searches the selected service and offers explicit play and library actions", async () => {
  const hit = {
    id: "remote",
    title: "Provider song",
    artist: "Provider artist",
    duration_ms: 63000,
  };
  const fetch = vi
    .fn()
    .mockResolvedValue(new Response(JSON.stringify({ items: [hit] })));
  vi.stubGlobal("fetch", skippingFeatured(fetch));
  render(createElement(DiscoverPage, props));
  expect(fetch).not.toHaveBeenCalled();
  expect(screen.queryByRole("group", { name: "Music service" })).toBeNull();
  expect(screen.getByPlaceholderText("Search Home collection")).toBeTruthy();
  fireEvent.change(screen.getByRole("searchbox", { name: "Search music" }), {
    target: { value: "song" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Search" }));
  expect(
    (
      (await screen.findByRole("button", {
        name: "Play Provider song",
      })) as HTMLButtonElement
    ).disabled,
  ).toBe(false);
  expect(
    (
      screen.getByRole("button", {
        name: "Add Provider song to library",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(false);
  expect(screen.getByText("1:03")).toBeTruthy();
  expect(player.prepareSelection).not.toHaveBeenCalled();
});

test("empty discovery leads directly to music-service settings", () => {
  render(createElement(DiscoverPage, { ...props, connections: [] }));
  expect(
    screen
      .getByRole("link", { name: "Connect a music service" })
      .getAttribute("href"),
  ).toBe("/settings?section=plugins");
  expect(screen.queryByRole("searchbox")).toBeNull();
});

test("the empty provider search is one compact row with a real button", () => {
  const { container } = render(
    createElement(DiscoverPage, { ...props, connections: [] }),
  );
  const block = container.querySelector(".discovery-empty") as HTMLElement;
  expect(block.querySelector(".empty")).toBeNull();
  expect(block.textContent).toContain(
    "Connect a music service to search its catalogue here.",
  );
  const action = within(block).getByRole("link", {
    name: "Connect a music service",
  });
  expect(action.className).toContain("button");
  expect(action.className).toContain("primary");
});

test("the panel says results stay outside the library and does not announce a side effect before it happens", () => {
  render(createElement(DiscoverPage, props));
  expect(screen.queryByText(/adds a copy/i)).toBeNull();
  expect(
    screen.getByText(
      /What you find stays outside your library until you add it/,
    ),
  ).toBeTruthy();
  expect(
    screen.getByRole("link", { name: "Manage plugins" }).getAttribute("href"),
  ).toBe("/settings?section=plugins");
});

test("switching named services clears results and preserves the search draft", async () => {
  const hit = {
    id: "same-remote",
    title: "Home song",
    artist: "Fixture artist",
  };
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(new Response(JSON.stringify({ items: [hit] })))
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify({ items: [{ ...hit, title: "Archive song" }] }),
      ),
    );
  vi.stubGlobal("fetch", skippingFeatured(fetch));
  render(
    createElement(DiscoverPage, {
      ...props,
      connections: [
        connection,
        { ...connection, id: "archive", service_id: "archive-service" },
      ],
      service_names: {
        ...props.service_names,
        "archive-service": "Archive collection",
      },
    }),
  );
  const search = screen.getByRole("searchbox", { name: "Search music" });
  fireEvent.change(search, { target: { value: "song" } });
  fireEvent.click(screen.getByRole("button", { name: "Search" }));
  await screen.findByRole("button", { name: "Play Home song" });
  fireEvent.click(
    within(screen.getByRole("group", { name: "Music service" })).getByRole(
      "button",
      { name: /Archive collection/ },
    ),
  );
  expect(screen.queryByRole("button", { name: "Play Home song" })).toBeNull();
  expect((search as HTMLInputElement).value).toBe("song");
  fireEvent.click(screen.getByRole("button", { name: "Search" }));
  await screen.findByRole("button", { name: "Play Archive song" });
  expect(fetch.mock.calls[1][0]).toBe("/api/v1/plugins/archive/search");
});

test("a failed selection offers service refresh without starting audio", async () => {
  const hit = { id: "remote", title: "Provider song", artist: "Fixture" };
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(new Response(JSON.stringify({ items: [hit] })))
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify({ error: "The plugin connection changed." }),
        { status: 409 },
      ),
    );
  vi.stubGlobal("fetch", skippingFeatured(fetch));
  render(createElement(DiscoverPage, props));
  fireEvent.change(screen.getByRole("searchbox", { name: "Search music" }), {
    target: { value: "song" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Search" }));
  fireEvent.click(
    await screen.findByRole("button", { name: "Play Provider song" }),
  );
  expect((await screen.findByRole("alert")).textContent).toContain("changed");
  expect(
    screen.getByRole("link", { name: "Refresh services" }).getAttribute("href"),
  ).toBe("/discover");
});

test("a failed search does not claim there are no matching tracks", async () => {
  vi.stubGlobal(
    "fetch",
    skippingFeatured(
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ error: "The service is unavailable." }), {
          status: 502,
        }),
      ),
    ),
  );
  render(createElement(DiscoverPage, props));
  fireEvent.change(screen.getByRole("searchbox", { name: "Search music" }), {
    target: { value: "song" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Search" }));
  expect((await screen.findByRole("alert")).textContent).toContain(
    "unavailable",
  );
  expect(
    screen.queryByRole("heading", { name: "No matching tracks" }),
  ).toBeNull();
});

test("a service without sources offers adding to the library but not playing", async () => {
  const hit = { id: "remote", title: "Provider song", artist: "Fixture" };
  vi.stubGlobal(
    "fetch",
    skippingFeatured(
      vi.fn().mockResolvedValue(new Response(JSON.stringify({ items: [hit] }))),
    ),
  );
  render(
    createElement(DiscoverPage, {
      ...props,
      connections: [
        {
          ...connection,
          manifest: {
            ...connection.manifest,
            capabilities: ["search", "metadata", "artwork"],
          },
        },
      ],
    }),
  );
  fireEvent.change(screen.getByRole("searchbox", { name: "Search music" }), {
    target: { value: "song" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Search" }));
  const add = (await screen.findByRole("button", {
    name: "Add Provider song to library",
  })) as HTMLButtonElement;
  expect(add.disabled).toBe(false);
  expect(
    (
      screen.getByRole("button", {
        name: "Play Provider song",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
  expect(screen.getByText(/provides no audio/)).toBeTruthy();
});

test("a result without artist or duration prints only what is known and keeps the duration cell", async () => {
  const fetch = vi.fn().mockResolvedValue(
    new Response(
      JSON.stringify({
        items: [
          { id: "a", title: "Bare hit", artist: "Unknown artist" },
          { id: "b", title: "Full hit", artist: "Known", duration_ms: 63000 },
        ],
      }),
    ),
  );
  vi.stubGlobal("fetch", skippingFeatured(fetch));
  const { container } = render(createElement(DiscoverPage, props));
  fireEvent.change(screen.getByRole("searchbox", { name: "Search music" }), {
    target: { value: "hit" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Search" }));
  await screen.findByRole("button", { name: "Play Bare hit" });
  expect(screen.queryByText(/Unknown artist/)).toBeNull();
  expect(screen.queryByText("—")).toBeNull();
  expect(
    screen.getByText(/^Home collection · not in your library$/),
  ).toBeTruthy();
  const rows = container.querySelectorAll(".provider-result");
  expect(rows).toHaveLength(2);
  for (const row of rows)
    expect(row.querySelectorAll(".provider-duration")).toHaveLength(1);
  expect(rows[0].querySelector(".provider-duration")!.textContent).toBe("");
  expect(rows[1].querySelector(".provider-duration")!.textContent).toBe("1:03");
});
