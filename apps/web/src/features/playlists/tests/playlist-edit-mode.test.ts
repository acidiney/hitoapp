import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import {
  act,
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

function open(page = props) {
  const sent: Schema<"SyncBatch">[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options: RequestInit) => {
      if (path === "/api/v1/sync") {
        const batch: Schema<"SyncBatch"> = JSON.parse(String(options.body));
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
        children: createElement(PlaylistPage, page),
      }),
    ),
  );
  return sent;
}

const actions = (sent: Schema<"SyncBatch">[]) =>
  sent.flatMap((batch) =>
    batch.operations.map((operation) => operation.action),
  );

const rowTitles = () =>
  within(screen.getByRole("table"))
    .getAllByRole("row")
    .slice(1)
    .map(
      (row) => row.querySelector(".track-title, .retained-title")?.textContent,
    );

test("entries are read-only until Edit is pressed", () => {
  open();
  expect(
    screen.queryByRole("button", { name: /^Remove .* from playlist$/ }),
  ).toBeNull();
  expect(screen.queryByRole("button", { name: /^Move .* down$/ })).toBeNull();
  expect(screen.queryByLabelText("Name")).toBeNull();
  expect(screen.queryByRole("button", { name: "Done" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  expect(screen.getByRole("button", { name: "Done" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Cancel" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Edit" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Save name" })).toBeNull();
  expect(screen.getByLabelText("Name")).toBeTruthy();
  expect(
    screen.getAllByRole("button", { name: "Remove Fixture A from playlist" }),
  ).toHaveLength(2);
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  expect(screen.queryByLabelText("Name")).toBeNull();
  expect(screen.getByRole("button", { name: "Edit" })).toBeTruthy();
});

test("Done without changes closes editing and sends nothing", () => {
  const sent = open();
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  expect(sent).toHaveLength(0);
  expect(router.reload).not.toHaveBeenCalled();
});

test("renaming is saved by Done as one rename and one reload", async () => {
  const sent = open();
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(screen.getByLabelText("Name"), {
    target: { value: "  Evening  " },
  });
  expect(sent).toHaveLength(0);
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
  expect(actions(sent)).toEqual([
    { kind: "playlist_rename", playlist_id: playlist, name: "Evening" },
  ]);
});

test("the name cannot be saved empty", () => {
  open();
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(screen.getByLabelText("Name"), { target: { value: " " } });
  expect(
    (screen.getByRole("button", { name: "Done" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
});

test("a name another playlist already uses gets a gentle warning and can still be saved", async () => {
  const sent = open({
    ...props,
    playlists: [
      { id: "other", name: "Evening", kind: "local" },
      { id: playlist, name: "Copied selection", kind: "local" },
    ],
  });
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  expect(document.querySelector(".name-warning")).toBeNull();
  fireEvent.change(screen.getByLabelText("Name"), {
    target: { value: "evening" },
  });
  expect(document.querySelector(".name-warning")?.textContent).toBe(
    "You already have a playlist called “evening”. You can still use this name.",
  );
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
  expect(actions(sent)).toHaveLength(1);
});

test("moving an entry changes the shown order but saves nothing until Done", async () => {
  const sent = open();
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  expect(rowTitles()).toEqual([
    "Fixture A",
    "Unknown provider item",
    "Fixture B",
    "Fixture A",
  ]);
  const rows = within(screen.getByRole("table")).getAllByRole("row").slice(1);
  expect(
    within(rows[0]).getByRole("button", { name: "Move Fixture A up" }),
  ).toHaveProperty("disabled", true);
  fireEvent.click(
    within(rows[0]).getByRole("button", { name: "Move Fixture A down" }),
  );
  expect(rowTitles()).toEqual([
    "Unknown provider item",
    "Fixture A",
    "Fixture B",
    "Fixture A",
  ]);
  expect(sent).toHaveLength(0);
  expect(router.reload).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
  expect(sent).toHaveLength(1);
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

test("Cancel without changes closes editing at once", () => {
  open();
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getByRole("button", { name: "Edit" })).toBeTruthy();
});

test("Cancel with changes asks first, then reverts the name, order and removals without saving", () => {
  const sent = open();
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(screen.getByLabelText("Name"), {
    target: { value: "Other" },
  });
  fireEvent.click(
    screen.getAllByRole("button", { name: "Move Fixture A down" })[0],
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Remove Fixture B from playlist" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  const dialog = within(
    screen.getByRole("dialog", { name: "Discard your changes?" }),
  );
  fireEvent.click(dialog.getByRole("button", { name: "Keep editing" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect((screen.getByLabelText("Name") as HTMLInputElement).value).toBe(
    "Other",
  );
  expect(rowTitles()[0]).toBe("Unknown provider item");
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  fireEvent.click(
    within(screen.getByRole("dialog")).getByRole("button", {
      name: "Discard changes",
    }),
  );
  expect(screen.getByRole("button", { name: "Edit" })).toBeTruthy();
  expect(rowTitles()).toEqual([
    "Fixture A",
    "Unknown provider item",
    "Fixture B",
    "Fixture A",
  ]);
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  expect((screen.getByLabelText("Name") as HTMLInputElement).value).toBe(
    props.detail.playlist.name,
  );
  expect(
    screen.getByRole("button", { name: "Remove Fixture B from playlist" }),
  ).toBeTruthy();
  expect(sent).toHaveLength(0);
  expect(router.reload).not.toHaveBeenCalled();
});

test("removing an entry marks it until Done and can be undone", async () => {
  const sent = open();
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.click(
    screen.getByRole("button", { name: "Remove Fixture B from playlist" }),
  );
  expect(sent).toHaveLength(0);
  expect(rowTitles()).toContain("Fixture B");
  fireEvent.click(
    screen.getByRole("button", { name: "Keep Fixture B in playlist" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  expect(sent).toHaveLength(0);
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.click(
    screen.getByRole("button", { name: "Remove Fixture B from playlist" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
  expect(actions(sent)).toEqual([
    { kind: "entry_remove", playlist_id: playlist, entry_id: second.entry_id },
  ]);
});

test("one Done saves the rename, removals and order together in one request", async () => {
  const sent = open();
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(screen.getByLabelText("Name"), {
    target: { value: "Mixed" },
  });
  fireEvent.click(
    screen.getAllByRole("button", { name: "Move Fixture A down" })[0],
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Remove Fixture B from playlist" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
  expect(sent).toHaveLength(1);
  expect(actions(sent).map((action) => action.kind)).toEqual([
    "playlist_rename",
    "entry_remove",
    "entry_move",
    "entry_move",
  ]);
});

test("a failed save keeps the editing state and the draft", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => Response.json({ error: "boom" }, { status: 500 })),
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
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.click(
    screen.getAllByRole("button", { name: "Move Fixture A down" })[0],
  );
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  await waitFor(() =>
    expect(
      (screen.getByRole("button", { name: "Done" }) as HTMLButtonElement)
        .disabled,
    ).toBe(false),
  );
  expect(router.reload).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Cancel" })).toBeTruthy();
  expect(rowTitles()[0]).toBe("Unknown provider item");
});

test("edit controls stay busy until the reload finishes", async () => {
  open();
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.click(
    screen.getAllByRole("button", { name: "Move Fixture A down" })[0],
  );
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
  const options = vi.mocked(router.reload).mock.calls[0][0] as {
    onFinish: () => void;
  };
  expect(
    (screen.getByRole("button", { name: "Done" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  expect(
    (screen.getByRole("button", { name: "Cancel" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  act(() => options.onFinish());
  expect(screen.getByRole("button", { name: "Edit" })).toBeTruthy();
});
