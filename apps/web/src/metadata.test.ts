// @vitest-environment jsdom
import { createElement, type ReactNode } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import { change } from "./api";
import { NoticeProvider } from "./components/Notices";
import RecordingTitleEditor from "./components/RecordingTitleEditor";
import { act, renderHook, waitFor } from "@testing-library/react";
import { useMetadataHistory } from "./hooks/useMetadataHistory";
import { metadataApi } from "./metadataApi";
import type { ObservationPage, RecordingMetadata } from "./types";

const player = vi.hoisted(() => ({ setRecordingTitle: vi.fn() }));
vi.mock("./player/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("./api", () => ({ change: vi.fn() }));
vi.mock("./metadataApi", () => ({ metadataApi: { recording: vi.fn() } }));
vi.mock("@inertiajs/react", () => ({ router: { reload: vi.fn() } }));
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

test("an accepted title reset updates the queue and reloads details", async () => {
  vi.mocked(change).mockResolvedValueOnce({ acknowledged: [] });
  render(
    createElement(RecordingTitleEditor, {
      account: "alice",
      recording: "track",
      title: {
        value: "My title",
        inherited: "Source title",
        personal: "My title",
        operation_id: "accepted",
      },
    }),
    { wrapper },
  );
  fireEvent.click(screen.getByRole("button", { name: "Use source title" }));
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
  expect(change).toHaveBeenCalledWith(
    "alice",
    { kind: "recording_title_inherit", recording_id: "track" },
    expect.any(AbortSignal),
  );
  expect(player.setRecordingTitle).toHaveBeenCalledWith(
    "track",
    null,
    "Source title",
  );
});

test("a late history response cannot appear after switching accounts", async () => {
  let resolve!: (value: RecordingMetadata) => void;
  vi.mocked(metadataApi.recording).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    }),
  );
  const alice: ObservationPage = { items: [], next_before: 20 };
  const bob: ObservationPage = { items: [], next_before: null };
  const { result, rerender } = renderHook(
    ({ account, initial }) => useMetadataHistory(account, "recording", initial),
    { initialProps: { account: "alice", initial: alice } },
  );
  let pending!: Promise<void>;
  act(() => {
    pending = result.current.older();
  });
  rerender({ account: "bob", initial: bob });
  await act(async () => {
    resolve({
      recording_id: "recording",
      title: {
        value: "Alice",
        inherited: "Source",
        personal: "Alice",
        operation_id: "private",
      },
      selected_observation: null,
      observations: { items: [], next_before: 8 },
    });
    await pending;
  });
  expect(result.current.page).toEqual(bob);
  expect(result.current.pending).toBe(false);
  expect(result.current.error).toBe("");
  expect(vi.mocked(metadataApi.recording).mock.calls[0][3].aborted).toBe(true);
});
const wrapper = ({ children }: { children: ReactNode }) =>
  createElement(NoticeProvider, null, children);

test("a failed title save preserves the draft and keeps the player unchanged", async () => {
  vi.mocked(change).mockRejectedValueOnce(new Error("Server unavailable"));
  render(
    createElement(RecordingTitleEditor, {
      account: "alice",
      recording: "track",
      title: {
        value: "Inherited",
        inherited: "Inherited",
        personal: null,
        operation_id: null,
      },
    }),
    { wrapper },
  );
  const input = screen.getByLabelText("Track title") as HTMLInputElement;
  fireEvent.change(input, { target: { value: "Keep my draft" } });
  fireEvent.click(screen.getByRole("button", { name: "Save personal title" }));
  await screen.findByText("Server unavailable");
  expect(input.value).toBe("Keep my draft");
  expect(player.setRecordingTitle).not.toHaveBeenCalled();
  expect(router.reload).not.toHaveBeenCalled();
});
