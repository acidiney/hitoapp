import type { SourceResolver, Source } from "../api/playbackSources";
import type { PlaybackOccurrence } from "../model";
import {
  BrowserPreparation,
  type BrowserMediaOwner,
} from "./BrowserPreparation";

type Following = {
  generation: number;
  currentRecording?: string;
  active: Source;
};

export class BrowserLookahead {
  private prepared: BrowserPreparation | null = null;
  private opening: BrowserPreparation | null = null;
  private revision = 0;

  constructor(
    private resolve: SourceResolver,
    private owner: BrowserMediaOwner,
  ) {}

  cancel() {
    this.revision++;
    this.prepared?.close();
    this.prepared = null;
  }

  prepare(occurrence: PlaybackOccurrence | undefined, following: Following) {
    const entry = occurrence?.id;
    const recording = occurrence?.recording;
    if (!entry || !recording) {
      this.cancel();
      return;
    }
    if (this.matches(this.prepared, occurrence, following.generation)) return;
    const seed =
      recording.id === following.currentRecording && following.active.ready
        ? { ...following.active }
        : undefined;
    this.cancel();
    this.prepared = new BrowserPreparation(
      {
        generation: following.generation,
        revision: this.revision,
        entry,
        recording: recording.id,
      },
      this.resolve,
      this.owner,
      seed,
    );
  }

  claim(occurrence: PlaybackOccurrence | undefined, generation: number) {
    const candidate = this.prepared;
    const claimed = this.matches(candidate, occurrence, generation)
      ? candidate
      : null;
    if (claimed) this.prepared = null;
    this.cancel();
    this.opening?.close();
    this.opening = claimed;
    return claimed;
  }

  isOpening(preparation: BrowserPreparation) {
    return this.opening === preparation;
  }

  settleOpening() {
    this.opening = null;
  }

  closeOpening() {
    this.opening?.close();
    this.opening = null;
  }

  private matches(
    candidate: BrowserPreparation | null,
    occurrence: PlaybackOccurrence | undefined,
    generation: number,
  ) {
    const target = candidate?.target;
    return (
      !!candidate &&
      target?.entry === occurrence?.id &&
      target?.recording === occurrence?.recording.id &&
      target?.generation === generation &&
      target?.revision === this.revision
    );
  }
}
