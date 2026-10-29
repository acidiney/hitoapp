import { createElement, type ReactNode } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistPage from "../pages/PlaylistPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import type { SyncBatch } from "../../../shared/types";
import { copiedPlaylistPage } from "../fixtures/playlistPages";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({
    children,
    href,
    ...attributes
  }: {
    children: ReactNode;
    href: string;
  }) => createElement("a", { href, ...attributes }, children),
}));

const props = {
  ...copiedPlaylistPage,
  detail: { ...copiedPlaylistPage.detail, origin: null },
};
const playlist = props.detail.playlist.id;
const [first, , second] = props.detail.entries;

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

function open() {
  const sent: SyncBatch[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options: RequestInit) => {
      if (path === "/api/v1/sync") {
        const batch: SyncBatch = JSON.parse(String(options.body));
        sent.push(batch);
        return Response.json({
          acknowledged: batch.operations.map((operation) => operation.id),
        });
      }
      return Response.json({ error: "Not found" }, { status: 404 });
    }),
  );
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: props.user.id,
        children: createElement(PlaylistPage, props),
      }),
    ),
  );
  return sent;
}

const actions = (sent: SyncBatch[]) =>
  sent.flatMap((batch) =>
    batch.operations.map((operation) => operation.action),
  );

test("entries are read-only until Edit is pressed", () => {
  open();
  expect(
    screen.queryByRole("button", { name: /^Remove .* from playlist$/ }),
  ).toBeNull();
  expect(screen.queryByRole("button", { name: /^Move .* down$/ })).toBeNull();
  expect(screen.queryByLabelText("Name")).toBeNull();
  const edit = screen.getByRole("button", { name: "Edit" });
  expect(edit.getAttribute("aria-pressed")).toBe("false");
  fireEvent.click(edit);
  expect(
    screen.getByRole("button", { name: "Done" }).getAttribute("aria-pressed"),
  ).toBe("true");
  expect(screen.getByLabelText("Name")).toBeTruthy();
  expect(
    screen.getAllByRole("button", { name: "Remove Fixture A from playlist" }),
  ).toHaveLength(2);
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  expect(screen.queryByLabelText("Name")).toBeNull();
});

test("renaming sends one rename and reloads", async () => {
  const sent = open();
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(screen.getByLabelText("Name"), {
    target: { value: "  Evening  " },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save name" }));
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
  expect(actions(sent)).toEqual([
    { kind: "playlist_rename", playlist_id: playlist, name: "Evening" },
  ]);
});

test("the name cannot be saved empty or unchanged", () => {
  open();
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  const save = screen.getByRole("button", {
    name: "Save name",
  }) as HTMLButtonElement;
  expect(save.disabled).toBe(true);
  fireEvent.change(screen.getByLabelText("Name"), { target: { value: " " } });
  expect(save.disabled).toBe(true);
});

test("moving an entry down swaps the two positions", async () => {
  const sent = open();
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  const rows = within(screen.getByRole("table")).getAllByRole("row").slice(1);
  expect(
    within(rows[0]).getByRole("button", { name: "Move Fixture A up" }),
  ).toHaveProperty("disabled", true);
  fireEvent.click(
    within(rows[0]).getByRole("button", { name: "Move Fixture A down" }),
  );
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
  expect(actions(sent)).toEqual([
    {
      kind: "entry_move",
      playlist_id: playlist,
      entry_id: props.detail.entries[1].entry_id,
      position: first.position,
    },
    {
      kind: "entry_move",
      playlist_id: playlist,
      entry_id: first.entry_id,
      position: props.detail.entries[1].position,
    },
  ]);
  expect(second).toBeDefined();
});

test("removing an entry in edit mode sends one removal", async () => {
  const sent = open();
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.click(
    screen.getByRole("button", { name: "Remove Fixture B from playlist" }),
  );
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
  expect(actions(sent)).toEqual([
    { kind: "entry_remove", playlist_id: playlist, entry_id: second.entry_id },
  ]);
});
