import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import { change } from "../../../shared/sync";
import { NoticeProvider } from "../../../shared/ui/Notices";
import MetadataEditor from "../components/MetadataEditor";
import MetadataObservations from "../components/MetadataObservations";
import { act, renderHook, waitFor } from "@testing-library/react";
import { useMetadataHistory } from "../hooks/useMetadataHistory";
import { metadataApi } from "../api/metadataApi";

import type {
  MetadataEditorInput,
  MetadataField,
  MetadataTarget,
} from "../model/metadata";

const player = vi.hoisted(() => ({ setRecordingField: vi.fn() }));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("../../../shared/sync", () => ({ change: vi.fn() }));
vi.mock("../api/metadataApi", () => ({ metadataApi: { history: vi.fn() } }));
vi.mock("@inertiajs/react", () => ({ router: { reload: vi.fn() } }));
test("provider observations show provider provenance without claiming file inspection", () => {
  render(
    createElement(MetadataObservations, {
      account: "alice",
      target: { kind: "recording", id: "track" },
      selected: "observation",
      initial: {
        next_before: null,
        items: [
          {
            id: "observation",
            source_id: null,
            release_id: null,
            provider: "org.hito.fixture",
            origin: "provider_response",
            observed_at: 1,
            inspection_version: null,
            plugin_installation_id: "connection",
            provider_recording_id: "remote-track",
            provider_revision: "revision-2",
            title: "Provider title",
            artist: "Provider artist",
            album: null,
            album_artist: null,
            duration_ms: 0,
            disc_number: null,
            track_number: null,
            tags: null,
          },
        ],
      },
    }),
  );
  expect(screen.getByText("remote-track")).toBeTruthy();
  expect(screen.getByText("revision-2")).toBeTruthy();
  expect(screen.getByText("Current metadata")).toBeTruthy();
  expect(screen.queryByText("Inspected tags")).toBeNull();
  expect(screen.queryByText("Imported during upgrade")).toBeNull();
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
  let resolve!: (value: Schema<"RecordingMetadata">) => void;
  vi.mocked(metadataApi.history).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    }),
  );
  const alice: Schema<"ObservationPage"> = { items: [], next_before: 20 };
  const bob: Schema<"ObservationPage"> = { items: [], next_before: null };
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
      credited_artists: [],
      musicbrainz_recording_id: null,
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
  let resolve!: (value: Schema<"RecordingMetadata">) => void;
  vi.mocked(metadataApi.history).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    }),
  );
  const initial: Schema<"ObservationPage"> = { items: [], next_before: 20 };
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
      credited_artists: [],
      musicbrainz_recording_id: null,
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

test("an unknown album artist can be personalized and reset without changing recording credits", async () => {
  const unknown: Schema<"ReleaseMetadata"> = {
    release_id: "source-free-album",
    title: {
      value: "Owned album",
      inherited: "Owned album",
      personal: null,
      operation_id: null,
    },
    artist: {
      value: null,
      inherited: null,
      personal: null,
      operation_id: null,
    },
    musicbrainz: { release_id: null, release_group_id: null },
    observations: { items: [], next_before: null },
  };
  const requiredRecording: Pick<
    Schema<"RecordingMetadata">,
    "title" | "artist"
  > = {
    title: {
      value: "Owned track",
      inherited: "Owned track",
      personal: null,
      operation_id: null,
    },
    artist: {
      value: "Required track artist",
      inherited: "Required track artist",
      personal: null,
      operation_id: null,
    },
  };
  const savedId = "00000000-0000-4000-8000-000000000001";
  const resetId = "00000000-0000-4000-8000-000000000002";
  vi.mocked(change)
    .mockResolvedValueOnce({ acknowledged: [savedId] })
    .mockResolvedValueOnce({ acknowledged: [resetId] });
  const finishReload = (options: Parameters<typeof router.reload>[0]) => {
    options?.onCancelToken?.({ cancel: () => {} });
    options?.onFinish?.({} as never);
  };
  vi.mocked(router.reload)
    .mockImplementationOnce(finishReload)
    .mockImplementationOnce(finishReload);
  const albumEditor = (metadata: Schema<"ReleaseMetadata">) =>
    createElement(MetadataEditor, {
      account: "alice",
      kind: "release",
      id: metadata.release_id,
      metadata,
    });
  const { rerender, unmount } = render(albumEditor(unknown), { wrapper });
  const artist = screen.getByLabelText<HTMLInputElement>("Album artist");
  expect(artist.value).toBe("");
  expect(artist.required).toBe(true);
  expect(
    screen.getByText("Not provided by source", { exact: true }),
  ).toBeTruthy();
  expect(
    screen.queryByRole("button", { name: "Use source artist" }),
  ).toBeNull();
  expect(
    screen.getByRole<HTMLButtonElement>("button", {
      name: "Save personal artist",
    }).disabled,
  ).toBe(true);
  fireEvent.change(artist, {
    target: { value: "Owned personal album credit" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save personal artist" }));
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
  expect(change).toHaveBeenNthCalledWith(
    1,
    "alice",
    {
      kind: "release_artist_set",
      release_id: unknown.release_id,
      artist: "Owned personal album credit",
    },
    expect.any(AbortSignal),
  );
  const personal: Schema<"ReleaseMetadata"> = {
    ...unknown,
    artist: {
      value: "Owned personal album credit",
      inherited: null,
      personal: "Owned personal album credit",
      operation_id: savedId,
    },
  };
  rerender(albumEditor(personal));
  expect(screen.getByLabelText<HTMLInputElement>("Album artist").value).toBe(
    "Owned personal album credit",
  );
  expect(
    screen.getByText("Not provided by source", { exact: true }),
  ).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Use source artist" }));
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(2));
  expect(change).toHaveBeenNthCalledWith(
    2,
    "alice",
    {
      kind: "release_artist_inherit",
      release_id: unknown.release_id,
    },
    expect.any(AbortSignal),
  );
  const reset: Schema<"ReleaseMetadata"> = {
    ...unknown,
    artist: {
      value: null,
      inherited: null,
      personal: null,
      operation_id: resetId,
    },
  };
  rerender(albumEditor(reset));
  expect(screen.getByLabelText<HTMLInputElement>("Album artist").value).toBe(
    "",
  );
  expect(
    screen.getByText("Not provided by source", { exact: true }),
  ).toBeTruthy();
  expect(
    screen.queryByRole("button", { name: "Use source artist" }),
  ).toBeNull();
  expect(reset.artist).toEqual({
    value: null,
    inherited: null,
    personal: null,
    operation_id: resetId,
  });
  expect(unknown.artist).toEqual({
    value: null,
    inherited: null,
    personal: null,
    operation_id: null,
  });
  expect(player.setRecordingField).not.toHaveBeenCalled();
  unmount();
  render(
    createElement(MetadataEditor, {
      account: "alice",
      kind: "recording",
      id: "track",
      metadata: requiredRecording,
    }),
    { wrapper },
  );
  const trackArtist = screen.getByLabelText<HTMLInputElement>("Track artist");
  expect(trackArtist.value).toBe("Required track artist");
  expect(trackArtist.required).toBe(true);
  expect(requiredRecording.artist.inherited).toBe("Required track artist");
  expect(change).toHaveBeenCalledTimes(2);
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
  metadata: Schema<"TextMetadata">;
}) {
  const inherited = {
    value: "Source",
    inherited: "Source",
    personal: null,
    operation_id: null,
  };
  const fields = { title: inherited, artist: inherited, [field]: metadata };
  const input: MetadataEditorInput =
    target.kind === "recording"
      ? { kind: "recording", id: target.id, metadata: fields }
      : { kind: "release", id: target.id, metadata: fields };
  return createElement(MetadataEditor, { account, ...input });
}
