import type { components } from "../../../generated";
import { request, requestJson } from "../../../shared/http";
import { readObservations } from "./observations";

type Schema = components["schemas"];
export type Snapshot = Schema["PlayerSnapshot"];
export type Position = Schema["PlayerPosition"];
export type Action = Schema["PlayerAction"];
export type Command = Schema["PlayerCommand"];
export type Receipt = Schema["PlayerReceipt"];
export type Attachment = Schema["PlayerAttachment"];
export type AudiobookRequest = Schema["PlayerAudiobookRequest"];
export type AudiobookContinuation = Schema["PlayerAudiobookContinuation"];

export const nativeApi = {
  async inspect(
    account: string,
    signal: AbortSignal,
  ): Promise<Schema["PlayerStatus"]> {
    return requestJson<Schema["PlayerStatus"]>("/player", {
      action: "native.inspect",
      account,
      signal,
    });
  },
  async claim(
    account: string,
    epoch: string,
    signal: AbortSignal,
  ): Promise<Attachment> {
    return requestJson<Attachment>("/player/claim", {
      action: "native.claim",
      method: "POST",
      account,
      signal,
      body: { epoch } satisfies Schema["PlayerClaim"],
    });
  },
  async command(
    account: string,
    body: Command,
    signal: AbortSignal,
  ): Promise<Receipt> {
    return requestJson<Receipt>("/player/commands", {
      action: "native.command",
      method: "POST",
      account,
      signal,
      body,
    });
  },
  async queue(
    account: string,
    body: Schema["PlayerQueueRequest"],
    signal: AbortSignal,
  ): Promise<Schema["PlayerQueuePage"]> {
    return requestJson<Schema["PlayerQueuePage"]>("/player/queue", {
      action: "native.queue",
      method: "POST",
      account,
      signal,
      body,
    });
  },
  async prepareAudiobook(
    account: string,
    body: AudiobookRequest,
    signal: AbortSignal,
  ): Promise<AudiobookContinuation> {
    return requestJson<AudiobookContinuation>("/player/audiobooks/prepare", {
      action: "native.audiobook_prepare",
      method: "POST",
      account,
      signal,
      body,
    });
  },
  async cancelAudiobook(
    account: string,
    body: Schema["PlayerAudiobookCancel"],
    signal: AbortSignal,
  ): Promise<void> {
    await request("/player/audiobooks/cancel", {
      action: "native.audiobook_cancel",
      method: "POST",
      account,
      signal,
      body,
    });
  },
  async settleAudiobook(
    account: string,
    body: Schema["PlayerAudiobookCancel"] & { command: Command },
    signal: AbortSignal,
  ): Promise<Schema["PlayerAudiobookSettlement"]> {
    return requestJson<Schema["PlayerAudiobookSettlement"]>(
      "/player/audiobooks/cancel",
      {
        action: "native.audiobook_settle",
        method: "POST",
        account,
        signal,
        body,
      },
    );
  },
  async observe<T extends Snapshot | Position>(
    account: string,
    lease: string,
    kind: "state" | "position",
    signal: AbortSignal,
    receive: (value: T) => void,
  ) {
    const response = await request(`/player/${kind}/events`, {
      action: `native.observe_${kind}`,
      method: "POST",
      account,
      signal,
      body: { lease } satisfies Schema["PlayerObservation"],
    });
    await readObservations<T>(response, signal, receive);
  },
};
