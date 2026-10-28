import type { components } from "../../../generated";

type Schema = components["schemas"];

export function audiobookPages() {
  const recordingA = "e3af308a-236b-4812-9166-b29910949025";
  const first: Schema["PlaylistEntry"] = {
    entry_id: "cf6d6fcb-1644-44b4-b49d-8ebd81e67250",
    recording_id: recordingA,
    position: 10,
    title: "Owned chapter A",
    inherited_title: "Owned chapter A",
    artist: "Owned narrator",
    inherited_artist: "Owned narrator",
    duration_ms: 120_000,
    liked: true,
    available: true,
  };
  const chapters: Schema["PlaylistEntry"][] = [
    first,
    {
      ...first,
      entry_id: "1813dcbd-9ce3-4b9d-bfb6-c70d3f4a564f",
      recording_id: "d74f9fb1-0f32-4724-99b7-4543dec2eb22",
      position: 20,
      title: "Owned chapter B",
      inherited_title: "Owned chapter B",
    },
    {
      ...first,
      entry_id: "b3ecda1c-e351-42d5-8916-0d0eb32ae081",
      position: 30,
    },
    {
      ...first,
      entry_id: "dd56c1d9-4738-4993-8cfc-76a65cabf77d",
      recording_id: null,
      position: 40,
      title: "Retained chapter",
      inherited_title: "Retained chapter",
      duration_ms: 0,
      liked: false,
      available: false,
      origin: {
        kind: "unavailable",
        occurrence_id: "retained-chapter",
        provider_recording_id: "owned-provider/retained-chapter",
      },
    },
  ];
  const playlist: Schema["Playlist"] = {
    id: "eb0f0af1-dc27-4737-ad8a-928b8bf6a32e",
    name: "An owned audiobook",
    kind: "local",
  };
  const shared: Schema["SharedPageProps"] = {
    errors: {},
    native_playback: false,
    lock: { pin_enabled: false, idle_minutes: 0, locked: false },
    user: { id: "alice", username: "Alice", admin: false },
    libraries: [],
    playlists: [
      playlist,
      { id: "unrelated-playlist", name: "Ungrouped selection", kind: "local" },
    ],
  };
  const bookmark: Schema["AudiobookBookmark"] = {
    id: "596e771e-b43c-4ea3-bc60-639561481247",
    audiobook_id: playlist.id,
    entry_id: chapters[2].entry_id,
    recording_id: recordingA,
    position_ms: 35_000,
    label: "Return to the repeated chapter",
  };
  const removedChapterBookmark: Schema["AudiobookBookmark"] = {
    ...bookmark,
    id: "fd6b32cb-9b31-4264-a37a-e8b15720c538",
    entry_id: "e343f0b0-daa8-4d47-97b6-813af9dc0f15",
    position_ms: 15_000,
    label: "Keep the removed chapter's place",
  };
  const index: Schema["AudiobookIndexPageProps"] = {
    ...shared,
    books: [playlist],
    book_pagination: { limit: 60, next_cursor: null, previous_cursor: null },
    grouping_playlists: shared.playlists.filter(
      (item) => item.id !== playlist.id,
    ),
  };
  const detail: Schema["AudiobookPageProps"] = {
    ...shared,
    detail: { playlist, chapters, bookmarks: [bookmark] },
  };
  return { index, detail, bookmark, removedChapterBookmark };
}

export function audiobookGrouping() {
  const { index, detail } = audiobookPages();
  const playlist: Schema["Playlist"] = {
    id: "a2a56629-f64a-4bd4-a3dd-83402ecba41c",
    name: "My chapter selection",
    kind: "local",
  };
  const followed: Schema["Playlist"] = {
    id: "19105821-69d1-4a4f-8817-c37f9ec4b410",
    name: "Followed chapters",
    kind: "followed",
  };
  const props: Schema["AudiobookIndexPageProps"] = {
    ...index,
    playlists: [
      index.books[0],
      {
        id: "6097548f-c19b-4543-988e-e250243c64db",
        name: "Ungrouped selection",
        kind: "local",
      },
      playlist,
      followed,
    ],
  };
  props.grouping_playlists = props.playlists.filter(
    (item) =>
      item.kind === "local" && !props.books.some((book) => book.id === item.id),
  );
  const confirmed: Schema["AudiobookDetail"] = {
    ...detail.detail,
    playlist,
    bookmarks: [],
  };
  return { index: props, playlist, followed, confirmed };
}

export function heldAudiobookBody(value: Schema["AudiobookDetail"]) {
  const bytes = new TextEncoder().encode(JSON.stringify(value));
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  let released = false;
  const response = new Response(
    new ReadableStream<Uint8Array>({
      start(stream) {
        controller = stream;
        stream.enqueue(bytes.slice(0, 1));
      },
      cancel() {
        released = true;
      },
    }),
    { headers: { "content-type": "application/json" } },
  );
  function release() {
    if (released) return;
    released = true;
    controller.enqueue(bytes.slice(1));
    controller.close();
  }
  return {
    response,
    release,
    async dispose() {
      release();
      if (!response.bodyUsed) await response.body?.cancel();
    },
  };
}
