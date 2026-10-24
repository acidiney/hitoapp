import { createUuid } from "../../../shared/uuid";
import type { PlaybackOccurrence, PlaybackQueueInput, Track } from "../model";

function occurrence(value: PlaybackQueueInput): PlaybackOccurrence {
  const recording = "recording" in value ? value.recording : value;
  const chapter = "recording" in value ? value.chapter : undefined;
  if (chapter && chapter.recording_id !== recording.id)
    throw new Error("The chapter does not match its recording.");
  return Object.freeze({
    id: createUuid(),
    recording: Object.freeze({ ...recording }),
    ...(chapter ? { chapter: Object.freeze({ ...chapter }) } : {}),
  });
}

export class BrowserQueue {
  private entries: PlaybackOccurrence[] = [];
  private ids: string[] = [];

  get length() {
    return this.entries.length;
  }
  identities(): readonly string[] {
    return this.ids;
  }
  recordings(): Track[] {
    return this.entries.map(({ recording }) => recording);
  }
  at(index: number) {
    return this.entries[index];
  }
  indexOf(id: string) {
    return this.ids.indexOf(id);
  }
  page(offset: number, limit: number) {
    return this.entries.slice(offset, offset + limit);
  }
  replace(input: PlaybackQueueInput[]) {
    this.entries = input.map(occurrence);
    this.ids = this.entries.map(({ id }) => id);
  }
  append(input: PlaybackQueueInput[]) {
    const added = input.map(occurrence);
    for (const entry of added) {
      this.entries.push(entry);
      this.ids.push(entry.id);
    }
    return added;
  }
  move(from: number, to: number) {
    const [entry] = this.entries.splice(from, 1);
    this.entries.splice(to, 0, entry);
    this.ids.splice(from, 1);
    this.ids.splice(to, 0, entry.id);
  }
  updateRecording(update: (recording: Track) => Track) {
    this.entries = this.entries.map((entry) =>
      Object.freeze({
        ...entry,
        recording: Object.freeze(update(entry.recording)),
      }),
    );
  }
  remove(index: number) {
    this.entries = this.entries.filter((_, position) => position !== index);
    this.ids = this.entries.map(({ id }) => id);
  }
  clear() {
    this.entries = [];
    this.ids = [];
  }
}
