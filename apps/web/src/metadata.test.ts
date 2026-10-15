// @vitest-environment jsdom
import { createElement, type ReactNode } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import { change } from "./api";
import { NoticeProvider } from "./components/Notices";
import MetadataEditor from "./components/MetadataEditor";
import { act, renderHook, waitFor } from "@testing-library/react";
import { useMetadataHistory } from "./hooks/useMetadataHistory";
import { metadataApi } from "./metadataApi";
import type { ObservationPage, RecordingMetadata, TextMetadata } from "./types";
import type { MetadataField, MetadataTarget } from "./metadata";

const player = vi.hoisted(() => ({ setRecordingField: vi.fn() }));
vi.mock("./player/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("./api", () => ({ change: vi.fn() }));
vi.mock("./metadataApi", () => ({ metadataApi: { history: vi.fn() } }));
vi.mock("@inertiajs/react", () => ({ router: { reload: vi.fn() } }));
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

test("an accepted title reset updates the queue and reloads details", async () => {
  vi.mocked(change).mockResolvedValueOnce({ acknowledged: [] });
  render(
    createElement(Editor, {
      account: "alice",
      target: { kind: "recording", id: "track" },
      field: "title",
      metadata: {
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
  expect(player.setRecordingField).toHaveBeenCalledWith(
    "track",
    "title",
    null,
    "Source title",
  );
});

test("a late history response cannot appear after switching accounts", async () => {
  let resolve!: (value: RecordingMetadata) => void;
  vi.mocked(metadataApi.history).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    }),
  );
  const alice: ObservationPage = { items: [], next_before: 20 };
  const bob: ObservationPage = { items: [], next_before: null };
  const { result, rerender } = renderHook(
    ({ account, initial }) =>
      useMetadataHistory(
        account,
        { kind: "recording", id: "recording" },
        initial,
      ),
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
      artist: {
        value: "Source",
        inherited: "Source",
        personal: null,
        operation_id: null,
      },
      observations: { items: [], next_before: 8 },
    });
    await pending;
  });
  expect(result.current.page).toEqual(bob);
  expect(result.current.pending).toBe(false);
  expect(result.current.error).toBe("");
  expect(vi.mocked(metadataApi.history).mock.calls[0][3].aborted).toBe(true);
});

test.each([
  [
    { kind: "recording", id: "track" },
    "artist",
    "Track artist",
    {
      kind: "recording_artist_set",
      recording_id: "track",
      artist: "Private credit",
    },
  ],
  [
    { kind: "release", id: "album" },
    "artist",
    "Album artist",
    {
      kind: "release_artist_set",
      release_id: "album",
      artist: "Private credit",
    },
  ],
  [
    { kind: "release", id: "album" },
    "title",
    "Album title",
    { kind: "release_title_set", release_id: "album", title: "Private credit" },
  ],
] as const)(
  "saving %s %s sends the right typed operation",
  async (target, field, label, action) => {
    vi.mocked(change).mockResolvedValueOnce({ acknowledged: [] });
    render(
      createElement(Editor, {
        account: "alice",
        target: target as MetadataTarget,
        field: field as MetadataField,
        metadata: {
          value: "Inherited",
          inherited: "Inherited",
          personal: null,
          operation_id: null,
        },
      }),
      { wrapper },
    );
    fireEvent.change(screen.getByLabelText(label), {
      target: { value: "Private credit" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: `Save personal ${field}` }),
    );
    await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
    expect(change).toHaveBeenCalledWith(
      "alice",
      action,
      expect.any(AbortSignal),
    );
    if (target.kind === "recording")
      expect(player.setRecordingField).toHaveBeenCalledWith(
        "track",
        field,
        "Private credit",
        "Inherited",
      );
    else expect(player.setRecordingField).not.toHaveBeenCalled();
  },
);
const wrapper = ({ children }: { children: ReactNode }) =>
  createElement(NoticeProvider, null, children);

test("a failed title save preserves the draft and keeps the player unchanged", async () => {
  vi.mocked(change).mockRejectedValueOnce(new Error("Server unavailable"));
  render(
    createElement(Editor, {
      account: "alice",
      target: { kind: "recording", id: "track" },
      field: "title",
      metadata: {
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
  expect(player.setRecordingField).not.toHaveBeenCalled();
  expect(router.reload).not.toHaveBeenCalled();
});

test("history requests cannot leak between recording and album targets with the same ID", async () => {
  let resolve!: (value: RecordingMetadata) => void;
  vi.mocked(metadataApi.history).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    }),
  );
  const initial: ObservationPage = { items: [], next_before: 20 };
  const { result, rerender } = renderHook(
    ({ kind }: { kind: "recording" | "release" }) =>
      useMetadataHistory("alice", { kind, id: "same" }, initial),
    { initialProps: { kind: "recording" } },
  );
  let pending!: Promise<void>;
  act(() => {
    pending = result.current.older();
  });
  rerender({ kind: "release" });
  await act(async () => {
    resolve({
      recording_id: "same",
      title: {
        value: "Title",
        inherited: "Title",
        personal: null,
        operation_id: null,
      },
      artist: {
        value: "Artist",
        inherited: "Artist",
        personal: null,
        operation_id: null,
      },
      selected_observation: null,
      observations: { items: [], next_before: 8 },
    });
    await pending;
  });
  expect(result.current.page).toEqual(initial);
  expect(vi.mocked(metadataApi.history).mock.calls[0][3].aborted).toBe(true);
});

function Editor({
  account,
  target,
  field,
  metadata,
}: {
  account: string;
  target: MetadataTarget;
  field: MetadataField;
  metadata: TextMetadata;
}) {
  const inherited = {
    value: "Source",
    inherited: "Source",
    personal: null,
    operation_id: null,
  };
  return createElement(MetadataEditor, {
    account,
    target,
    metadata: { title: inherited, artist: inherited, [field]: metadata },
  });
}
