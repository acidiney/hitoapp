import type { components } from "../../generated";
import { request } from "../../http";
import { readObservations } from "./observations";

type Schema = components["schemas"];
export type Snapshot = Schema["PlayerSnapshot"];
export type Position = Schema["PlayerPosition"];
export type Action = Schema["PlayerAction"];
export type Command = Schema["PlayerCommand"];
export type Receipt = Schema["PlayerReceipt"];
export type Attachment = Schema["PlayerAttachment"];

export const nativeApi = {
  async inspect(
    account: string,
    signal: AbortSignal,
  ): Promise<Schema["PlayerStatus"]> {
    return (
      await request("/player", { action: "native.inspect", account, signal })
    ).json();
  },
  async claim(
    account: string,
    epoch: string,
    signal: AbortSignal,
  ): Promise<Attachment> {
    return (
      await request("/player/claim", {
        action: "native.claim",
        method: "POST",
        account,
        signal,
        body: { epoch } satisfies Schema["PlayerClaim"],
      })
    ).json();
  },
  async command(
    account: string,
    body: Command,
    signal: AbortSignal,
  ): Promise<Receipt> {
    return (
      await request("/player/commands", {
        action: "native.command",
        method: "POST",
        account,
        signal,
        body,
      })
    ).json();
  },
  async queue(
    account: string,
    body: Schema["PlayerQueueRequest"],
    signal: AbortSignal,
  ): Promise<Schema["PlayerQueuePage"]> {
    return (
      await request("/player/queue", {
        action: "native.queue",
        method: "POST",
        account,
        signal,
        body,
      })
    ).json();
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
