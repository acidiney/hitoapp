import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import PodcastIndexPage from "../pages/PodcastIndexPage";
import PodcastPage from "../pages/PodcastPage";
import { episodeState } from "../model/episodeState";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { openSubscribeDialog } from "../../../test/media";
import type { components } from "../../../generated";

type Schema = components["schemas"];
const playQueue = vi.fn(async () => {});
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({
    playQueue,
    subscribe: () => () => {},
    snapshot: () => ({ current: null }),
  }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { visit: vi.fn(), reload: vi.fn() },
  Link: ({
    children,
    href,
    ...attributes
  }: {
    children: ReactNode;
    href: string;
  }) => createElement("a", { href, ...attributes }, children),
}));
afterEach(() => {
  cleanup();
  playQueue.mockClear();
});

function episode(id: string, title: string): Schema["PodcastEpisode"] {
  return {
    episode_id: `${id}-episode`,
    recording: {
      id,
      library_id: "library",
      title,
      inherited_title: title,
      artist: "The host",
      inherited_artist: "The host",
      duration_ms: 3_600_000,
      liked: false,
      available: true,
    },
  };
}

function page(progress: Schema["RecordingProgress"][]) {
  const items = [episode("rec-new", "Newest"), episode("rec-mid", "Halfway")];
  return {
    user: { id: "alice", username: "Alice", admin: false },
    errors: {},
    native_playback: false,
    lock: { pin_enabled: false, idle_minutes: 0, locked: false },
    libraries: [],
    playlists: [],
    subscription: {
      subscription_id: "3668fe94-ed9b-4508-a3dc-bb051e889950",
      title: "Salt of the Earth",
      refresh_pending: false,
      refresh_failed: false,
      last_refreshed_at: 1_790_000_000,
      content_revision: 1,
    },
    episodes: {
      items,
      total: 2,
      limit: 50,
      next_cursor: null,
      content_revision: 1,
    },
    progress,
  } satisfies Schema["PodcastPageProps"];
}

const view = (progress: Schema["RecordingProgress"][]) =>
  render(createElement(PodcastPage, page(progress)), {
    wrapper: ({ children }) => createElement(NoticeProvider, null, children),
  });

test("episode state is New, Remaining or Played from duration and saved position", () => {
  expect(episodeState(3_600_000, 0)).toMatchObject({ kind: "new" });
  expect(episodeState(3_600_000, 1_800_000)).toMatchObject({
    kind: "remaining",
    percent: 50,
    label: "30 min left",
  });
  expect(episodeState(3_600_000, 3_599_000)).toMatchObject({
    kind: "played",
    percent: 100,
  });
});

test("the podcast header continues the episode in progress and the list shows state, progress and actions", () => {
  view([{ recording_id: "rec-mid", position_ms: 1_800_000, speed: 1 }]);
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
    "Salt of the Earth",
  );
  expect(screen.getByText("Following")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Refresh feed" })).toBeTruthy();
  expect(
    screen.getByRole("heading", { name: "For this podcast" }),
  ).toBeTruthy();
  expect(screen.getByRole("button", { name: "Unsubscribe" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: /Download/ })).toBeNull();
  expect(screen.queryByRole("link", { name: /Download/ })).toBeNull();

  const list = screen.getByRole("list", { name: "Episodes" });
  const rows = within(list).getAllByRole("listitem");
  expect(rows).toHaveLength(2);
  expect(within(rows[0]).getByText("New · 1 h 0 min")).toBeTruthy();
  expect(within(rows[1]).getByText("30 min left")).toBeTruthy();
  expect(
    within(rows[1])
      .getByRole("progressbar", { name: "Progress of Halfway" })
      .getAttribute("aria-valuenow"),
  ).toBe("50");
  expect(
    within(rows[1])
      .getByRole("link", { name: "Details for Halfway" })
      .getAttribute("href"),
  ).toBe("/recordings/rec-mid");

  fireEvent.click(
    screen.getByRole("button", { name: "Continue · 30 min left" }),
  );
  expect(playQueue).toHaveBeenCalledWith(
    [
      expect.objectContaining({ id: "rec-new" }),
      expect.objectContaining({ id: "rec-mid" }),
    ],
    1,
    { kind: "saved", position: 1800 },
  );
});

test("without progress the header plays the latest episode and the filter hides played episodes", () => {
  view([{ recording_id: "rec-mid", position_ms: 3_599_500, speed: 1 }]);
  expect(screen.queryByRole("button", { name: /^Continue/ })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Play latest" }));
  expect(playQueue).toHaveBeenCalledWith(expect.any(Array), 0, {
    kind: "saved",
    position: 0,
  });

  const list = screen.getByRole("list", { name: "Episodes" });
  expect(within(list).getAllByRole("listitem")).toHaveLength(1);
  expect(screen.getByRole("radio", { name: "Unplayed" })).toBeTruthy();
  expect(screen.queryByRole("radio", { name: "Downloaded" })).toBeNull();
  fireEvent.click(screen.getByRole("radio", { name: "All" }));
  const rows = within(
    screen.getByRole("list", { name: "Episodes" }),
  ).getAllByRole("listitem");
  expect(rows).toHaveLength(2);
  expect(within(rows[1]).getByText("Played")).toBeTruthy();
});

test("the podcast index shows cards that link to each podcast and an empty state", async () => {
  const props = {
    user: { id: "alice", username: "Alice", admin: false },
    errors: {},
    native_playback: false,
    lock: { pin_enabled: false, idle_minutes: 0, locked: false },
    libraries: [],
    playlists: [],
    subscriptions: [],
  } satisfies Schema["PodcastIndexPageProps"];
  const subscription = {
    subscription_id: "515993a6-776f-42bc-9d28-d4df99ceae28",
    title: "Salt of the Earth",
    refresh_pending: false,
    refresh_failed: true,
    last_refreshed_at: 1_790_000_000,
    content_revision: 1,
  } satisfies Schema["PodcastSubscription"];
  let list: Schema["PodcastSubscription"][] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (_input: RequestInfo | URL, options?: RequestInit) =>
      options?.method === "POST"
        ? Response.json(
            { error: "This feed address is not allowed." },
            { status: 400 },
          )
        : Response.json(list),
    ),
  );
  const mounted = render(createElement(PodcastIndexPage, props), {
    wrapper: ({ children }) => createElement(NoticeProvider, null, children),
  });
  await act(async () => {});
  expect(
    screen.getByRole("heading", { level: 1, name: "Podcasts" }),
  ).toBeTruthy();
  expect(
    screen.getByRole("heading", { name: "No podcast subscriptions yet" }),
  ).toBeTruthy();
  expect(screen.queryByRole("dialog")).toBeNull();

  openSubscribeDialog();
  const dialog = screen.getByRole("dialog", { name: "Subscribe to a podcast" });
  fireEvent.change(
    within(dialog).getByRole("textbox", { name: "Feed address" }),
    {
      target: { value: "https://feed.invalid/x.xml" },
    },
  );
  fireEvent.click(within(dialog).getByRole("button", { name: "Subscribe" }));
  expect((await within(dialog).findByRole("alert")).textContent).toMatch(
    /rejected/,
  );
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();

  list = [subscription];
  mounted.unmount();
  render(
    createElement(PodcastIndexPage, {
      ...props,
      subscriptions: [subscription],
    }),
    {
      wrapper: ({ children }) => createElement(NoticeProvider, null, children),
    },
  );
  await act(async () => {});
  const cards = within(
    screen.getByRole("list", { name: "Subscribed podcasts" }),
  ).getAllByRole("listitem");
  expect(cards).toHaveLength(1);
  expect(
    within(cards[0])
      .getByRole("link", { name: "Salt of the Earth" })
      .getAttribute("href"),
  ).toBe(`/podcasts/${subscription.subscription_id}`);
  expect(within(cards[0]).getByText(/Last checked/)).toBeTruthy();
  expect(within(cards[0]).getByText("Last feed check failed")).toBeTruthy();
});

test("each episode row has one details link and a played row plays from the start", () => {
  view([{ recording_id: "rec-mid", position_ms: 3_599_500, speed: 1 }]);
  fireEvent.click(screen.getByRole("radio", { name: "All" }));
  const rows = within(
    screen.getByRole("list", { name: "Episodes" }),
  ).getAllByRole("listitem");
  for (const row of rows)
    expect(
      within(row)
        .getAllByRole("link")
        .filter((link) => link.getAttribute("href")?.startsWith("/recordings/")),
    ).toHaveLength(1);
  expect(within(rows[1]).queryByRole("progressbar")).not.toBeNull();
  fireEvent.click(within(rows[1]).getByRole("button", { name: "Play Halfway" }));
  expect(playQueue).toHaveBeenLastCalledWith(expect.any(Array), 1, {
    kind: "position",
    position: 0,
  });
});

test("a new episode has no progress bar and the check mark is decorative", () => {
  view([]);
  const rows = within(
    screen.getByRole("list", { name: "Episodes" }),
  ).getAllByRole("listitem");
  expect(within(rows[0]).queryByRole("progressbar")).toBeNull();
  expect(screen.getByText("Following").textContent).toBe("Following ✓");
  expect(document.querySelector(".tint-status [aria-hidden]")?.textContent).toBe(
    "✓",
  );
});

test("closing the dialog during an unconfirmed request keeps a retry next to the trigger", async () => {
  const props = {
    user: { id: "alice", username: "Alice", admin: false },
    errors: {},
    native_playback: false,
    lock: { pin_enabled: false, idle_minutes: 0, locked: false },
    libraries: [],
    playlists: [],
    subscriptions: [],
  } satisfies Schema["PodcastIndexPageProps"];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (_input: RequestInfo | URL, options?: RequestInit) => {
      if (options?.method === "POST") throw new TypeError("lost");
      return Response.json([]);
    }),
  );
  render(createElement(PodcastIndexPage, props), {
    wrapper: ({ children }) => createElement(NoticeProvider, null, children),
  });
  await act(async () => {});
  openSubscribeDialog();
  const dialog = screen.getByRole("dialog");
  fireEvent.change(within(dialog).getByRole("textbox", { name: "Feed address" }), {
    target: { value: "https://feed.invalid/x.xml" },
  });
  fireEvent.click(within(dialog).getByRole("button", { name: "Subscribe" }));
  await within(dialog).findByRole("alert");
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getByRole("status").textContent).toMatch(/not confirmed/i);
  expect(screen.getByRole("button", { name: "Review subscription request" })).toBeTruthy();
});
