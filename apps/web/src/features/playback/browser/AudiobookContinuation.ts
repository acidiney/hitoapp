import type { components } from "../../../generated";
import type { PlaybackQueueInput } from "../model";

export type AudiobookPlayback = components["schemas"]["AudiobookPlayback"];
export type AudiobookSelection = {
  queue: PlaybackQueueInput[];
  index: number;
  position: number;
  speed: number;
};

export function prepareAudiobookSelection(
  value: AudiobookPlayback,
  book: string,
): AudiobookSelection {
  const uuid = /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/;
  const identity = (value: unknown) =>
    typeof value === "string" &&
    uuid.test(value) &&
    value !== "00000000-0000-0000-0000-000000000000";
  if (
    !value ||
    value.audiobook_id !== book ||
    !identity(book) ||
    !value.chapter ||
    value.chapter.audiobook_id !== book ||
    !identity(value.chapter.entry_id) ||
    !identity(value.chapter.recording_id) ||
    !value.progress ||
    value.progress.recording_id !== value.chapter.recording_id ||
    !Number.isSafeInteger(value.progress.position_ms) ||
    value.progress.position_ms < 0 ||
    !Number.isFinite(value.progress.speed) ||
    value.progress.speed < 0.25 ||
    value.progress.speed > 4 ||
    !Array.isArray(value.chapters) ||
    !value.chapters.length ||
    value.chapters.length > 10000
  )
    throw new Error("The saved audiobook playback is invalid.");
  const entries = new Set<string>();
  const queue: PlaybackQueueInput[] = [];
  let index = -1;
  for (const chapter of value.chapters) {
    if (
      !chapter ||
      !identity(chapter.entry_id) ||
      entries.has(chapter.entry_id) ||
      !identity(chapter.recording_id) ||
      typeof chapter.available !== "boolean" ||
      typeof chapter.liked !== "boolean" ||
      typeof chapter.title !== "string" ||
      typeof chapter.artist !== "string" ||
      typeof chapter.inherited_title !== "string" ||
      typeof chapter.inherited_artist !== "string" ||
      !Number.isSafeInteger(chapter.duration_ms) ||
      chapter.duration_ms < 0
    )
      throw new Error("The audiobook chapter queue is invalid.");
    entries.add(chapter.entry_id);
    if (chapter.entry_id === value.chapter.entry_id) {
      if (
        chapter.recording_id !== value.chapter.recording_id ||
        !chapter.available
      )
        throw new Error("The saved audiobook chapter is unavailable.");
      index = queue.length;
    }
    queue.push({
      recording: { ...chapter, id: chapter.recording_id! },
      chapter: {
        audiobook_id: book,
        entry_id: chapter.entry_id,
        recording_id: chapter.recording_id!,
      },
    });
  }
  if (index < 0) throw new Error("The saved audiobook chapter is unavailable.");
  return {
    queue,
    index,
    position: value.progress.position_ms / 1000,
    speed: value.progress.speed,
  };
}
