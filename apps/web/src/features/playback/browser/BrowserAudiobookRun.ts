import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import type { QueueView } from "../model";
import {
  prepareAudiobookSelection,
  type AudiobookPlayback,
  type AudiobookSelection,
} from "./AudiobookContinuation";

export interface AudiobookHost {
  generation(): number;
  selection(): number;
  claimSelection(): number;
  queueView(): QueueView;
  persist(): boolean;
  open(target: AudiobookSelection): Promise<void>;
  fail(message: string): void;
}

export type AudiobookPreparer = (
  book: string,
  signal: AbortSignal,
) => Promise<AudiobookPlayback>;

export class BrowserAudiobookRun {
  private controller: AbortController | null = null;

  constructor(
    private host: AudiobookHost,
    private prepareAudiobook?: AudiobookPreparer,
  ) {}

  cancel() {
    this.controller?.abort();
    this.controller = null;
  }

  async run(book: string, signal: AbortSignal): Promise<boolean> {
    this.cancel();
    const controller = new AbortController();
    this.controller = controller;
    const combined = AbortSignal.any([signal, controller.signal]);
    const generation = this.host.generation();
    const selection = this.host.claimSelection();
    const queue = this.host.queueView();
    const started = performance.now();
    const requestId = createUuid();
    let timedOut = false;
    const deadline = setTimeout(() => {
      timedOut = true;
      controller.abort();
    }, 10_000);
    let cancel!: () => void;
    const cancelled = new Promise<never>((_, reject) => {
      cancel = () => reject(new DOMException("Cancelled", "AbortError"));
      combined.addEventListener("abort", cancel, { once: true });
    });
    const check = () => {
      combined.throwIfAborted();
      if (
        this.controller !== controller ||
        generation !== this.host.generation() ||
        selection !== this.host.selection() ||
        queue !== this.host.queueView()
      )
        throw new DOMException("Superseded", "AbortError");
    };
    let committed = false;
    log.debug("playback.audiobook_continue_started", { request_id: requestId });
    try {
      check();
      if (!this.prepareAudiobook)
        throw new Error("Audiobook continuation requires a server connection.");
      if (!this.host.persist())
        throw new Error("Listening progress is waiting to be saved.");
      const fresh = await Promise.race([
        this.prepareAudiobook(book, combined),
        cancelled,
      ]);
      check();
      const target = prepareAudiobookSelection(fresh, book);
      if (!this.host.persist())
        throw new Error("Listening progress is waiting to be saved.");
      check();
      clearTimeout(deadline);
      this.controller = null;
      committed = true;
      await this.host.open(target);
      return true;
    } catch (error) {
      if (timedOut && this.controller === controller && !signal.aborted) {
        this.host.fail("Continuing this audiobook timed out. Try again.");
        return false;
      }
      if (
        combined.aborted ||
        (error instanceof DOMException && error.name === "AbortError")
      )
        return false;
      this.host.fail(
        "Hito could not continue this audiobook. Check its saved chapter and playback speed, then try again.",
      );
      return false;
    } finally {
      clearTimeout(deadline);
      combined.removeEventListener("abort", cancel);
      if (this.controller === controller) this.controller = null;
      log.debug("playback.audiobook_continue_finished", {
        request_id: requestId,
        reason: committed
          ? "committed"
          : timedOut
            ? "timeout"
            : combined.aborted
              ? "cancelled"
              : "rejected",
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }
}
