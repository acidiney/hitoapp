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
  private numbers: number[] = [];
  private serialOf = new Map<string, number>();
  private issued = 0;

  get length() {
    return this.entries.length;
  }
  serials(): readonly number[] {
    return this.numbers;
  }
  serialAt(index: number): number | undefined {
    return this.numbers[index];
  }
  recordings(): Track[] {
    return this.entries.map(({ recording }) => recording);
  }
  at(index: number) {
    return this.entries[index];
  }
  indexOf(id: string) {
    const serial = this.serialOf.get(id);
    return serial === undefined ? -1 : this.numbers.indexOf(serial);
  }
  page(offset: number, limit: number) {
    return this.entries.slice(offset, offset + limit);
  }
  replace(input: PlaybackQueueInput[]) {
    this.entries = [];
    this.numbers = [];
    this.serialOf.clear();
    this.append(input);
  }
  append(input: PlaybackQueueInput[]) {
    const added = input.map(occurrence);
    for (const entry of added) {
      this.entries.push(entry);
      this.numbers.push(++this.issued);
      this.serialOf.set(entry.id, this.issued);
    }
    return added;
  }
  move(from: number, to: number) {
    const [entry] = this.entries.splice(from, 1);
    this.entries.splice(to, 0, entry);
    const [serial] = this.numbers.splice(from, 1);
    this.numbers.splice(to, 0, serial);
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
    const [entry] = this.entries.splice(index, 1);
    this.numbers.splice(index, 1);
    this.serialOf.delete(entry.id);
  }
  clear() {
    this.entries = [];
    this.numbers = [];
    this.serialOf.clear();
  }
}
