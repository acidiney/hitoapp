import type { Schema } from "../../shared/schema";
import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import { accountsApi } from "../../features/accounts/api/accountsApi";
import { change } from "../../shared/sync";
import { libraryApi } from "../../features/library/api/libraryApi";
import { NoticeBanner, NoticeProvider } from "../../shared/ui/Notices";
import PlaylistIndexPage from "../../features/playlists/pages/PlaylistIndexPage";
import { openCreate } from "../../test/playlistDialogs";
import PlaylistPage from "../../features/playlists/pages/PlaylistPage";
import { useMutation } from "../../shared/hooks/useMutation";
import { useScanJob } from "../../features/library/hooks/useScanJob";
import { useAccounts } from "../../features/accounts/hooks/useAccounts";

import { deferred } from "../../test/async";

vi.mock("../../shared/sync", () => ({ change: vi.fn() }));
vi.mock("../../features/library/api/libraryApi", () => ({
  libraryApi: { scan: vi.fn(), job: vi.fn() },
}));
vi.mock("../../features/accounts/api/accountsApi", () => ({
  accountsApi: { list: vi.fn(), create: vi.fn(), setLibraryAccess: vi.fn() },
}));
vi.mock("../../features/playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({ snapshot: () => ({ shuffle: false }) }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

function wrapper({ children }: { children: ReactNode }) {
  return createElement(
    NoticeProvider,
    null,
    children,
    createElement(NoticeBanner),
  );
}

const shared = {
  errors: {},
  native_playback: false,
  lock: { pin_enabled: false, idle_minutes: 0, locked: false },
  user: { id: "alice", username: "Alice", admin: true },
  libraries: [],
  playlists: [],
  connections: [],
  service_names: {},
};
beforeEach(() => vi.clearAllMocks());
test("failed playlist creation preserves its form and does not reload", async () => {
  vi.mocked(change).mockRejectedValueOnce(new Error("Server unavailable"));
  render(createElement(PlaylistIndexPage, shared), { wrapper });
  openCreate();
  const input = screen.getByRole("textbox", {
    name: "Name",
  }) as HTMLInputElement;
  fireEvent.change(input, { target: { value: "Keep this name" } });
  fireEvent.click(screen.getByRole("button", { name: "Create playlist" }));
  await screen.findByText("Server unavailable");
  expect(input.value).toBe("Keep this name");
  expect(router.reload).not.toHaveBeenCalled();
});

test("failed playlist deletion stays on the current page", async () => {
  vi.mocked(change).mockRejectedValueOnce(new Error("Delete failed"));
  render(
    createElement(PlaylistPage, {
      ...shared,
      detail: {
        playlist: { id: "playlist", name: "Keep me", kind: "local" },
        entries: [],
      },
    }),
    { wrapper },
  );
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Delete playlist" }));
  await screen.findByText("Delete failed");
  expect(router.visit).not.toHaveBeenCalled();
  expect(screen.getByRole("heading", { name: "Keep me" })).toBeTruthy();
});

test("only an accepted playlist creation closes its dialog", async () => {
  vi.mocked(change).mockResolvedValueOnce({ acknowledged: [] });
  render(createElement(PlaylistIndexPage, shared), { wrapper });
  openCreate();
  fireEvent.change(screen.getByRole("textbox", { name: "Name" }), {
    target: { value: "Saved name" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Create playlist" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

test("an old account mutation cannot show an error or clear the new account's pending state", async () => {
  const old = deferred<Schema<"SyncReceipt">>();
  const current = deferred<Schema<"SyncReceipt">>();
  vi.mocked(change)
    .mockReturnValueOnce(old.promise)
    .mockReturnValueOnce(current.promise);
  const { result, rerender } = renderHook(
    ({ account }) => useMutation(account),
    { initialProps: { account: "alice" }, wrapper },
  );
  let previous!: ReturnType<typeof result.current.mutate>;
  act(() => {
    previous = result.current.mutate({
      kind: "playlist_delete",
      playlist_id: "old",
    });
  });
  rerender({ account: "bob" });
  expect(result.current.pending).toBe(false);
  let next!: ReturnType<typeof result.current.mutate>;
  act(() => {
    next = result.current.mutate({
      kind: "playlist_delete",
      playlist_id: "new",
    });
  });
  await act(async () => {
    old.reject(new Error("Alice private failure"));
    await previous;
  });
  expect(screen.queryByText("Alice private failure")).toBeNull();
  expect(result.current.pending).toBe(true);
  expect(vi.mocked(change).mock.calls[0][2]?.aborted).toBe(true);
  await act(async () => {
    current.resolve({ acknowledged: [] });
    await next;
  });
  expect(result.current.pending).toBe(false);
});

test("switching a scan account or library cancels stale completion and allows a new scan", async () => {
  const old = deferred<Schema<"Job">[]>();
  const finished: Schema<"Job"> = {
    id: "new-job",
    library_id: "library",
    folder_id: "folder",
    kind: "scan",
    state: "complete",
    progress: 2,
    error: null,
  };
  vi.mocked(libraryApi.scan)
    .mockReturnValueOnce(old.promise)
    .mockResolvedValueOnce([{ ...finished, state: "queued" }]);
  vi.mocked(libraryApi.job).mockResolvedValue(finished);
  const { result, rerender } = renderHook(
    ({ account, library }) => useScanJob(account, library),
    { initialProps: { account: "alice", library: "old-library" }, wrapper },
  );
  let previous!: Promise<void>;
  act(() => {
    previous = result.current.scan();
  });
  rerender({ account: "bob", library: "new-library" });
  expect(result.current.scanning).toBe(false);
  await act(async () => {
    await result.current.scan();
  });
  expect(libraryApi.scan).toHaveBeenCalledTimes(2);
  expect(router.reload).toHaveBeenCalledTimes(1);
  await act(async () => {
    old.resolve([{ ...finished, id: "old-job" }]);
    await previous;
  });
  expect(router.reload).toHaveBeenCalledTimes(1);
  expect(libraryApi.job).toHaveBeenCalledTimes(1);
  expect(vi.mocked(libraryApi.scan).mock.calls[0][2]?.aborted).toBe(true);
});

test("unmounting a scan ignores a delayed job response", async () => {
  const pending = deferred<Schema<"Job">[]>();
  vi.mocked(libraryApi.scan).mockReturnValueOnce(pending.promise);
  const { result, unmount } = renderHook(() => useScanJob("alice", "library"), {
    wrapper,
  });
  let scanning!: Promise<void>;
  act(() => {
    scanning = result.current.scan();
  });
  unmount();
  await act(async () => {
    pending.resolve([
      {
        library_id: "library",
        folder_id: "folder",
        id: "job",
        kind: "scan",
        state: "queued",
        progress: 0,
        error: null,
      },
    ]);
    await scanning;
  });
  expect(libraryApi.job).not.toHaveBeenCalled();
  expect(router.reload).not.toHaveBeenCalled();
});

test("a library scan waits for every folder before reporting completion", async () => {
  const second = deferred<Schema<"Job">>();
  const first: Schema<"Job"> = {
    id: "first",
    library_id: "library",
    folder_id: "one",
    kind: "scan",
    state: "complete",
    progress: 2,
    error: null,
  };
  vi.mocked(libraryApi.scan).mockResolvedValueOnce([
    { ...first, state: "queued" },
    { ...first, id: "second", folder_id: "two", state: "queued" },
  ]);
  vi.mocked(libraryApi.job).mockImplementation(async (_account, id) => {
    if (id === "first") return first;
    if (id === "second") return second.promise;
    throw new Error("Unknown folder scan");
  });
  const { result } = renderHook(() => useScanJob("alice", "library"), {
    wrapper,
  });
  let scanning!: Promise<void>;
  act(() => {
    scanning = result.current.scan();
  });
  await waitFor(() => expect(libraryApi.job).toHaveBeenCalledTimes(2));
  expect(result.current.scanning).toBe(true);
  expect(router.reload).not.toHaveBeenCalled();
  await act(async () => {
    second.resolve({ ...first, id: "second", folder_id: "two", progress: 3 });
    await scanning;
  });
  expect(router.reload).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("status").textContent).toContain("5 audio files");
});

test("account management ignores a creation response after changing accounts", async () => {
  const old = deferred<{ id: string; username: string; admin: boolean }>();
  vi.mocked(accountsApi.list).mockResolvedValue([]);
  vi.mocked(accountsApi.create).mockReturnValueOnce(old.promise);
  const { result, rerender } = renderHook(
    ({ account }) => useAccounts(account),
    { initialProps: { account: "alice" }, wrapper },
  );
  await act(async () => {});
  let creating!: Promise<string | null>;
  act(() => {
    creating = result.current.create("private-account", "private-password");
  });
  rerender({ account: "bob" });
  await act(async () => {});
  await act(async () => {
    old.resolve({
      id: "private-id",
      username: "private-account",
      admin: false,
    });
    await creating;
  });
  expect(result.current.users).toEqual([]);
  expect(screen.queryByText(/Created account private-account/)).toBeNull();
});

test("React strict effect cleanup does not cancel subsequent account mutations", async () => {
  vi.mocked(change).mockResolvedValueOnce({ acknowledged: [] });
  const { result } = renderHook(() => useMutation("alice"), {
    reactStrictMode: true,
    wrapper,
  });
  let accepted!: Awaited<ReturnType<typeof result.current.mutate>>;
  await act(async () => {
    accepted = await result.current.mutate({
      kind: "playlist_delete",
      playlist_id: "playlist",
    });
  });
  expect(accepted.ok).toBe(true);
  expect(vi.mocked(change).mock.calls[0][2]?.aborted).toBe(false);
});
