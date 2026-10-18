// @vitest-environment jsdom
import {
  createElement,
  StrictMode,
  type MouseEvent,
  type ReactNode,
} from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import AudiobookIndexPage from "./pages/AudiobookIndexPage";
import { audiobookPages } from "./fixtures/audiobooks";
import { log } from "./log";

const boundaries = vi.hoisted(() => ({
  visit: vi.fn(),
  audio: vi.fn(),
  request: vi.fn(),
}));
vi.mock("@inertiajs/react", () => ({
  router: { visit: boundaries.visit },
  Link: ({
    children,
    href,
    ...options
  }: {
    children: ReactNode;
    href: string;
  }) =>
    createElement(
      "a",
      {
        href,
        onClick: (event: MouseEvent<HTMLAnchorElement>) => {
          event.preventDefault();
          boundaries.visit(href, options);
        },
      },
      children,
    ),
}));
beforeEach(() => {
  boundaries.audio.mockImplementation(() => {
    throw new Error("Library navigation must not open audio");
  });
  boundaries.request.mockRejectedValue(
    new Error("Library links must not fetch media or provider data"),
  );
  vi.stubGlobal("Audio", boundaries.audio);
  vi.stubGlobal("fetch", boundaries.request);
});
afterEach(() => {
  cleanup();
  expect(boundaries.audio).not.toHaveBeenCalled();
  expect(boundaries.request).not.toHaveBeenCalled();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
  vi.restoreAllMocks();
});

test("the audiobook index exposes stable links to the next page and back to the first page", () => {
  const { index } = audiobookPages();
  const props = Object.assign({}, index, {
    book_pagination: {
      limit: 60,
      next_cursor: "next-owned-cursor",
      previous_cursor: "previous-owned-cursor",
    },
    grouping_playlists: index.playlists.filter(
      (playlist) => playlist.id !== index.books[0].id,
    ),
  });
  render(createElement(AudiobookIndexPage, props));
  expect(
    screen.getByRole("link", { name: "Next audiobooks" }).getAttribute("href"),
  ).toBe("/audiobooks?cursor=next-owned-cursor");
  expect(
    screen
      .getByRole("link", { name: "Previous audiobooks" })
      .getAttribute("href"),
  ).toBe("/audiobooks?cursor=previous-owned-cursor");
  expect(
    screen.getByRole("link", { name: "First audiobooks" }).getAttribute("href"),
  ).toBe("/audiobooks");
});

test("a lost audiobook page navigation keeps its exact retry link without starting media", () => {
  const { index } = audiobookPages();
  const props = Object.assign({}, index, {
    book_pagination: {
      limit: 60,
      next_cursor: "next-owned-cursor",
      previous_cursor: null,
    },
  });
  render(createElement(AudiobookIndexPage, props));
  fireEvent.click(screen.getByRole("link", { name: "Next audiobooks" }));
  const [href, callbacks] = boundaries.visit.mock.calls[0];
  expect(href).toBe("/audiobooks?cursor=next-owned-cursor");
  act(() => {
    expect(callbacks.onBefore?.()).toBe(true);
    callbacks.onNetworkError?.(new TypeError("owned interrupted request"));
    callbacks.onFinish?.();
  });
  expect(screen.getByRole("alert").textContent).toContain("could not load");
  expect(
    screen
      .getByRole("link", { name: "Retry audiobook page" })
      .getAttribute("href"),
  ).toBe(href);
  expect(screen.getByRole("link", { name: index.books[0].name })).toBeTruthy();
});

test("StrictMode keeps audiobook page links active after effect replay", () => {
  const { index } = audiobookPages();
  render(
    createElement(
      StrictMode,
      null,
      createElement(AudiobookIndexPage, {
        ...index,
        book_pagination: {
          limit: 60,
          next_cursor: "strict-owned-cursor",
          previous_cursor: null,
        },
      }),
    ),
  );
  fireEvent.click(screen.getByRole("link", { name: "Next audiobooks" }));
  const [, callbacks] = boundaries.visit.mock.calls[0];
  act(() => expect(callbacks.onBefore()).toBe(true));
  expect(screen.getByRole("status").textContent).toBe("Loading audiobooks…");
});

test("grouped books outside the current page are absent from the authoritative grouping choices", () => {
  const { index } = audiobookPages();
  const elsewhere = {
    id: "outside-current-page",
    name: "An already grouped book on another page",
    kind: "local" as const,
  };
  render(
    createElement(AudiobookIndexPage, {
      ...index,
      playlists: [...index.playlists, elsewhere],
    }),
  );
  expect(screen.queryByRole("option", { name: elsewhere.name })).toBeNull();
  expect(screen.queryByRole("link", { name: elsewhere.name })).toBeNull();
  expect(
    screen.getByRole("option", { name: "Ungrouped selection" }),
  ).toBeTruthy();
});

test("a stalled page visit expires and retry keeps the exact bounded page URL", () => {
  vi.useFakeTimers();
  const { index } = audiobookPages();
  render(
    createElement(AudiobookIndexPage, {
      ...index,
      book_pagination: {
        limit: 20,
        next_cursor: "owned-stalled-cursor",
        previous_cursor: null,
      },
    }),
  );
  fireEvent.click(screen.getByRole("link", { name: "Next audiobooks" }));
  const [href, callbacks] = boundaries.visit.mock.calls[0];
  expect(href).toBe("/audiobooks?limit=20&cursor=owned-stalled-cursor");
  expect(callbacks.headers).toEqual({
    "x-hito-account": index.user.id,
    "x-hito-request": "1",
  });
  const cancel = vi.fn();
  act(() => {
    expect(callbacks.onBefore()).toBe(true);
    callbacks.onCancelToken({ cancel });
  });
  act(() => vi.advanceTimersByTime(10_000));
  expect(cancel).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("status")).toBeNull();
  expect(screen.getByRole("alert")).toBeTruthy();
  expect(
    screen
      .getByRole("link", { name: "Retry audiobook page" })
      .getAttribute("href"),
  ).toBe(href);
  expect(
    screen.getByRole("link", { name: "First audiobooks" }).getAttribute("href"),
  ).toBe("/audiobooks?limit=20");
  act(() => {
    callbacks.onSuccess({ component: "AudiobookIndexPage", props: index });
    callbacks.onFinish();
  });
  expect(screen.getByRole("alert")).toBeTruthy();
});

test.each(["account", "rendered page"])(
  "%s replacement retires held page visits and their late callbacks",
  (replacement) => {
    const { index } = audiobookPages();
    const props = {
      ...index,
      book_pagination: {
        limit: 60,
        next_cursor: "old-owned-cursor",
        previous_cursor: null,
      },
    };
    const view = render(createElement(AudiobookIndexPage, props));
    fireEvent.click(screen.getByRole("link", { name: "Next audiobooks" }));
    const [, callbacks] = boundaries.visit.mock.calls[0];
    const cancel = vi.fn();
    act(() => {
      expect(callbacks.onBefore()).toBe(true);
      callbacks.onCancelToken({ cancel });
    });
    const current = {
      ...index,
      user:
        replacement === "account"
          ? { ...index.user, id: "replacement-owner" }
          : index.user,
      book_pagination: {
        limit: 60,
        next_cursor: "new-owned-cursor",
        previous_cursor: null,
      },
    };
    view.rerender(createElement(AudiobookIndexPage, current));
    expect(cancel).toHaveBeenCalledTimes(1);
    act(() => {
      callbacks.onNetworkError(new Error("retired"));
      callbacks.onSuccess({ component: "AudiobookIndexPage", props: index });
      callbacks.onFinish();
    });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByRole("status")).toBeNull();
    expect(
      screen
        .getByRole("link", { name: "Next audiobooks" })
        .getAttribute("href"),
    ).toBe("/audiobooks?cursor=new-owned-cursor");
  },
);

test("one audiobook page attempt correlates safe logs without retaining private names or cursors", () => {
  const events: unknown[] = [];
  for (const level of ["info", "warn", "debug"] as const)
    vi.spyOn(log, level).mockImplementation((event, fields) =>
      events.push({ event, fields }),
    );
  const { index } = audiobookPages();
  render(
    createElement(AudiobookIndexPage, {
      ...index,
      books: [{ ...index.books[0], name: "private-book-log-canary" }],
      book_pagination: {
        limit: 60,
        next_cursor: "private-cursor-log-canary",
        previous_cursor: null,
      },
    }),
  );
  fireEvent.click(screen.getByRole("link", { name: "Next audiobooks" }));
  const [, callbacks] = boundaries.visit.mock.calls[0];
  act(() => {
    expect(callbacks.onBefore()).toBe(true);
    callbacks.onNetworkError(new TypeError("owned lost visit"));
    callbacks.onFinish();
  });
  expect(events).toHaveLength(3);
  const fields = events.map(
    (event) => (event as { fields: { request_id: string } }).fields,
  );
  expect(fields[0].request_id).toMatch(
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i,
  );
  expect(
    fields.every((field) => field.request_id === fields[0].request_id),
  ).toBe(true);
  expect(JSON.stringify(events)).not.toContain("private-book-log-canary");
  expect(JSON.stringify(events)).not.toContain("private-cursor-log-canary");
});
