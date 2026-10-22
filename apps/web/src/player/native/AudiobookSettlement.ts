import { HttpError } from "../../shared/http";
import { log } from "../../shared/log";
import {
  nativeApi,
  type AudiobookRequest,
  type Command,
  type Receipt,
} from "./api";
import { ObservationError } from "./observations";

export async function settleAudiobook(
  account: string,
  input: AudiobookRequest,
  command: Command,
): Promise<Receipt | null> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 10_000);
  let abort!: () => void;
  const expired = new Promise<never>((_, reject) => {
    abort = () =>
      reject(
        new HttpError(504, "Audiobook cancellation could not be confirmed."),
      );
    controller.signal.addEventListener("abort", abort, { once: true });
  });
  let retries = 0;
  try {
    while (true) {
      try {
        const settled = await Promise.race([
          nativeApi.settleAudiobook(
            account,
            { lease: input.lease, id: input.id, command },
            controller.signal,
          ),
          expired,
        ]);
        if (
          !settled ||
          typeof settled !== "object" ||
          !("receipt" in settled) ||
          (settled.receipt !== null &&
            (!settled.receipt || typeof settled.receipt !== "object"))
        )
          throw new ObservationError();
        return settled.receipt;
      } catch (error) {
        if (
          controller.signal.aborted ||
          error instanceof ObservationError ||
          (error instanceof HttpError &&
            error.status !== 429 &&
            error.status < 500)
        )
          throw error;
        log.debug("native.audiobook_settlement_retry", {
          operation_id: command.id,
          count: ++retries,
        });
        await Promise.race([
          new Promise((resolve) =>
            setTimeout(resolve, Math.min(250 * retries, 1000)),
          ),
          expired,
        ]);
      }
    }
  } finally {
    clearTimeout(timer);
    controller.signal.removeEventListener("abort", abort);
  }
}
