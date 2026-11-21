import { createElement, type AnchorHTMLAttributes } from "react";
import { render } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import Shell from "../Shell";

const locked = vi.hoisted(() => ({ value: false }));

vi.mock("@inertiajs/react", () => ({
  usePage: () => ({
    component: "CatalogPage",
    url: "/library",
    props: {
      user: { id: "alice", username: "Alice", admin: false },
      errors: {},
      native_playback: false,
      libraries: [],
      playlists: [],
      lock: { pin_enabled: false, idle_minutes: 0, locked: locked.value },
    },
  }),
  router: { get: vi.fn(), reload: vi.fn(), clearHistory: vi.fn() },
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

class CountingSource {
  static opened = 0;
  static closed = 0;
  onmessage = null;
  onerror = null;
  constructor() {
    CountingSource.opened++;
  }
  close() {
    CountingSource.closed++;
  }
}

test("the shell holds one change stream for the page and closes it on unmount", () => {
  CountingSource.opened = 0;
  CountingSource.closed = 0;
  vi.stubGlobal("EventSource", CountingSource);
  const view = render(createElement(Shell, null, "page"));
  view.rerender(createElement(Shell, null, "page again"));
  expect(CountingSource.opened).toBe(1);
  view.unmount();
  expect(CountingSource.closed).toBe(1);
});

test("a locked session opens no change stream", () => {
  CountingSource.opened = 0;
  locked.value = true;
  vi.stubGlobal("EventSource", CountingSource);
  render(createElement(Shell, null, "page"));
  expect(CountingSource.opened).toBe(0);
  locked.value = false;
});
