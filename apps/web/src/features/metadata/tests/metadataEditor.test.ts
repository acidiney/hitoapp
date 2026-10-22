import { createElement } from "react";
import {
  act,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import { change } from "../../../shared/sync";
import MetadataEditor from "../components/MetadataEditor";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { useMetadataEditor } from "../hooks/useMetadataEditor";

vi.mock("../../../shared/sync", () => ({ change: vi.fn() }));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({ setRecordingField: vi.fn() }),
}));
vi.mock("@inertiajs/react", () => ({ router: { reload: vi.fn() } }));
beforeEach(() => {
  vi.mocked(router.reload).mockImplementation((options) => {
    options?.onCancelToken?.({
      cancel: () => {
        options?.onFinish?.({} as never);
      },
    });
  });
});
const inherited = {
  value: "Source",
  inherited: "Source",
  personal: null,
  operation_id: null,
};
const metadata = { title: inherited, artist: inherited };

test("a pending field save protects the other draft from a concurrent reload", async () => {
  let resolve!: (value: { acknowledged: string[] }) => void;
  vi.mocked(change).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    }),
  );
  const inherited = {
    value: "Source",
    inherited: "Source",
    personal: null,
    operation_id: null,
  };
  render(
    createElement(MetadataEditor, {
      account: "alice",
      kind: "recording",
      id: "track",
      metadata: { title: inherited, artist: inherited },
    }),
    { wrapper: NoticeProvider },
  );
  const title = screen.getByLabelText("Track title") as HTMLInputElement;
  const artist = screen.getByLabelText("Track artist") as HTMLInputElement;
  fireEvent.change(artist, { target: { value: "Unsaved artist" } });
  fireEvent.change(title, { target: { value: "Personal title" } });
  fireEvent.click(screen.getByRole("button", { name: "Save personal title" }));
  expect(artist.disabled).toBe(true);
  expect(screen.getAllByRole("button", { name: "Saving…" })).toHaveLength(2);
  resolve({ acknowledged: [] });
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
  expect(artist.disabled).toBe(true);
  expect(artist.value).toBe("Unsaved artist");
  expect(change).toHaveBeenCalledTimes(1);
  await act(async () => {
    vi.mocked(router.reload).mock.calls[0][0]?.onFinish?.({} as never);
  });
  expect(artist.disabled).toBe(false);
});

test("target changes cancel a reload and its late finish cannot unlock a newer save", async () => {
  const cancels: ReturnType<typeof vi.fn>[] = [];
  vi.mocked(router.reload).mockImplementation((options) => {
    const cancel = vi.fn(() => options?.onFinish?.({} as never));
    cancels.push(cancel);
    options?.onCancelToken?.({ cancel });
  });
  vi.mocked(change).mockResolvedValue({ acknowledged: [] });
  const { result, rerender, unmount } = renderHook(
    ({ id }) => useMetadataEditor("alice", { kind: "recording", id, metadata }),
    {
      initialProps: { id: "old" },
      wrapper: NoticeProvider,
    },
  );
  await act(async () => {
    await result.current.save("title", "Saved title");
  });
  expect(result.current.pending).toBe(true);
  rerender({ id: "new" });
  expect(cancels[0]).toHaveBeenCalledTimes(1);
  expect(result.current.pending).toBe(false);
  await act(async () => {
    await result.current.save("artist", "Saved artist");
  });
  expect(result.current.pending).toBe(true);
  act(() => {
    vi.mocked(router.reload).mock.calls[0][0]?.onFinish?.({} as never);
  });
  expect(result.current.pending).toBe(true);
  unmount();
  expect(cancels[1]).toHaveBeenCalledTimes(1);
});

test("an old target's delayed acceptance cannot reload the new editor", async () => {
  let resolve!: (value: { acknowledged: string[] }) => void;
  vi.mocked(change).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    }),
  );
  const { result, rerender } = renderHook(
    ({ id }) => useMetadataEditor("alice", { kind: "recording", id, metadata }),
    {
      initialProps: { id: "old" },
      wrapper: NoticeProvider,
    },
  );
  let saving!: ReturnType<typeof result.current.save>;
  act(() => {
    saving = result.current.save("title", "Saved title");
  });
  rerender({ id: "new" });
  await act(async () => {
    resolve({ acknowledged: [] });
    expect((await saving).ok).toBe(false);
  });
  expect(router.reload).not.toHaveBeenCalled();
  expect(result.current.pending).toBe(false);
});

test.each(["throw", "prevent"])(
  "a reload that cannot start releases the editor after %s",
  async (failure) => {
    vi.mocked(change).mockResolvedValueOnce({ acknowledged: [] });
    vi.mocked(router.reload).mockImplementationOnce(() => {
      if (failure === "throw") throw new Error("Refresh failed");
    });
    const { result } = renderHook(
      () =>
        useMetadataEditor("alice", {
          kind: "recording",
          id: "track",
          metadata,
        }),
      { wrapper: NoticeProvider },
    );
    await act(async () => {
      const saved = await result.current.save("title", "Saved");
      expect(saved.ok).toBe(failure !== "throw");
    });
    expect(result.current.pending).toBe(false);
  },
);
