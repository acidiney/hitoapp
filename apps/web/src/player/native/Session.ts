import { HttpError } from "../../http";
import { log } from "../../log";
import {
  nativeApi,
  type Action,
  type Attachment,
  type Command,
  type Position,
  type Receipt,
  type Snapshot,
} from "./api";
import { ObservationError } from "./observations";

export type QueueSelection = {
  recordings: string[];
  start: number;
  position_ms: number;
};
export type Failure = "ownership" | "unavailable" | "protocol";
type Events = {
  state(value: Snapshot): void;
  position(value: Position): void;
  failure(kind: Failure): void;
  connection?(ready: boolean): void;
};

export class NativeSession {
  private controller = new AbortController();
  private attachment: Attachment | null = null;
  private sequence = 0;
  private revision = 0;
  private pending = 0;
  private retainedIds = 0;
  private lane = 0;
  private tail: Promise<unknown> = Promise.resolve();
  private opening: Promise<void> | null = null;
  private healthy = { state: false, position: false };

  constructor(
    private account: string,
    private events: Events,
  ) {}

  open(): Promise<void> {
    return (this.opening ??= this.attach());
  }
  private async attach() {
    const signal = this.controller.signal;
    const inspected = await nativeApi.inspect(this.account, signal);
    signal.throwIfAborted();
    if (!inspected.native || !inspected.epoch)
      throw new HttpError(501, "Native playback is unavailable.");
    const attachment = await nativeApi.claim(
      this.account,
      inspected.epoch,
      signal,
    );
    signal.throwIfAborted();
    if (!attachment.native)
      throw new HttpError(501, "Native playback is unavailable.");
    this.attachment = attachment;
    this.revision = attachment.state.revision;
    this.events.state(attachment.state);
    void this.observe("state");
    void this.observe("position");
    log.info("native.attached", { account_id: this.account });
  }

  submit(actions: Action[]): Promise<Receipt> {
    const intent = structuredClone(actions);
    const weight = intent.reduce(
      (count, action) =>
        count +
        (action.kind === "replace" || action.kind === "append"
          ? action.recordings.length
          : 0),
      0,
    );
    return this.admit(intent.length, weight, async (send) => {
      let receipt: Receipt | undefined;
      for (const action of intent) receipt = await send(action);
      return receipt!;
    });
  }

  replace(selection: QueueSelection): Promise<Receipt> {
    const recordings = selection.recordings.slice();
    const { start, position_ms } = selection;
    if (
      !recordings.length ||
      recordings.length > 100000 ||
      !Number.isInteger(start) ||
      start < 0 ||
      start >= recordings.length ||
      !Number.isSafeInteger(position_ms) ||
      position_ms < 0
    )
      return Promise.reject(new Error("The playback selection is invalid."));
    const count =
      Math.ceil(recordings.length / 1000) +
      2 +
      Number(recordings.length > 1000) +
      Number(position_ms > 0);
    return this.admit(count, recordings.length, async (send) => {
      await send({ kind: "pause" });
      await send({
        kind: "replace",
        recordings: recordings.slice(0, 1000),
        start: start < 1000 ? start : 0,
      });
      for (let offset = 1000; offset < recordings.length; offset += 1000)
        await send({
          kind: "append",
          recordings: recordings.slice(offset, offset + 1000),
        });
      if (recordings.length > 1000) {
        const page = await this.queue(this.revision, start, 1);
        const entry = page.entries[0];
        if (!entry || entry.recording.id !== recordings[start])
          throw new ObservationError();
        await send({ kind: "select", entry: entry.id });
      }
      if (position_ms) await send({ kind: "seek", position_ms });
      return send({ kind: "resume" });
    });
  }

  private admit(
    count: number,
    weight: number,
    work: (send: (action: Action) => Promise<Receipt>) => Promise<Receipt>,
  ): Promise<Receipt> {
    if (
      !count ||
      count > 128 ||
      this.pending >= 32 ||
      this.retainedIds + weight > 100000 ||
      !Number.isSafeInteger(this.sequence + count) ||
      this.controller.signal.aborted
    )
      return Promise.reject(
        new Error("Playback commands are unavailable. Try again."),
      );
    const tickets = Array.from({ length: count }, () => ({
      id: crypto.randomUUID(),
      sequence: ++this.sequence,
    }));
    const lane = this.lane;
    this.pending++;
    this.retainedIds += weight;
    const accepted = this.tail
      .catch(() => {})
      .then(async () => {
        await this.open();
        let cursor = 0;
        return work(async (action) => {
          this.controller.signal.throwIfAborted();
          if (lane !== this.lane)
            throw new DOMException("Superseded", "AbortError");
          return this.send(action, tickets[cursor++]);
        });
      });
    this.tail = accepted;
    return accepted
      .catch((error) => {
        if (lane === this.lane) this.lane++;
        throw error;
      })
      .finally(() => {
        this.pending--;
        this.retainedIds -= weight;
      });
  }

  async queue(
    revision: number,
    offset: number,
    limit: number,
    signal?: AbortSignal,
  ) {
    await this.open();
    const combined = signal
      ? AbortSignal.any([this.controller.signal, signal])
      : this.controller.signal;
    combined.throwIfAborted();
    return nativeApi.queue(
      this.account,
      { lease: this.attachment!.lease, revision, offset, limit },
      combined,
    );
  }

  private async send(
    action: Action,
    ticket: { id: string; sequence: number },
  ): Promise<Receipt> {
    const signal = this.controller.signal;
    const command: Command = {
      ...ticket,
      action,
      lease: this.attachment!.lease,
      revision: this.revision,
    };
    let retry = 0;
    while (!signal.aborted) {
      try {
        const receipt = await nativeApi.command(this.account, command, signal);
        signal.throwIfAborted();
        if (
          receipt.id !== command.id ||
          receipt.sequence !== command.sequence ||
          !Number.isSafeInteger(receipt.revision) ||
          receipt.revision < command.revision
        )
          throw new ObservationError();
        this.revision = Math.max(this.revision, receipt.revision);
        return receipt;
      } catch (error) {
        signal.throwIfAborted();
        if (error instanceof ObservationError) {
          this.fail("protocol");
          throw error;
        }
        if (
          error instanceof HttpError &&
          [401, 403, 409].includes(error.status)
        ) {
          this.fail("ownership");
          throw error;
        }
        if (
          error instanceof HttpError &&
          error.status !== 429 &&
          (error.status < 500 || error.status === 501)
        )
          throw error;
        log.warn("native.command_retry", {
          account_id: this.account,
          operation_id: command.id,
          count: ++retry,
        });
        await pause(Math.min(250 * retry, 2000), signal);
      }
    }
    throw new DOMException("Aborted", "AbortError");
  }

  private async observe(kind: "state" | "position") {
    const signal = this.controller.signal;
    while (!signal.aborted) {
      try {
        await nativeApi.observe<Snapshot | Position>(
          this.account,
          this.attachment!.lease,
          kind,
          signal,
          (value) => {
            if (signal.aborted) return;
            if (kind === "state") {
              const state = value as Snapshot;
              this.revision = Math.max(this.revision, state.revision);
              this.events.state(state);
            } else this.events.position(value as Position);
            if (!this.healthy[kind]) {
              this.healthy[kind] = true;
              if (this.healthy.state && this.healthy.position)
                this.events.connection?.(true);
            }
          },
        );
        if (signal.aborted) return;
      } catch (error) {
        if (signal.aborted) return;
        if (error instanceof ObservationError) {
          this.fail("protocol");
          return;
        }
        if (
          error instanceof HttpError &&
          [401, 403, 409].includes(error.status)
        ) {
          this.fail("ownership");
          return;
        }
      }
      this.healthy[kind] = false;
      this.events.connection?.(false);
      this.events.failure("unavailable");
      log.debug("native.observation_reconnect", {
        account_id: this.account,
        action: kind,
      });
      try {
        await pause(500, signal);
      } catch {
        return;
      }
    }
  }

  private fail(kind: Failure) {
    this.close();
    this.events.failure(kind);
  }
  close() {
    this.controller.abort();
    this.attachment = null;
    log.debug("native.detached", { account_id: this.account });
  }
}

function pause(milliseconds: number, signal: AbortSignal): Promise<void> {
  signal.throwIfAborted();
  return new Promise((resolve, reject) => {
    const abort = () => {
      clearTimeout(timer);
      reject(new DOMException("Aborted", "AbortError"));
    };
    const timer = setTimeout(() => {
      signal.removeEventListener("abort", abort);
      resolve();
    }, milliseconds);
    signal.addEventListener("abort", abort, { once: true });
  });
}
