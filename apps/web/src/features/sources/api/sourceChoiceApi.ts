import type { Schema } from "../../../shared/schema";
import { createUuid } from "../../../shared/uuid";
import { HttpError, request } from "../../../shared/http";
import { log } from "../../../shared/log";

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const identity = (value: unknown): value is string =>
  typeof value === "string" &&
  uuid.test(value) &&
  value !== "00000000-0000-0000-0000-000000000000";
const object = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === "object" && !Array.isArray(value);
const revision = (value: unknown): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const path = (recording: string) =>
  `/recordings/${encodeURIComponent(recording)}/source-choice`;

function validChoice(
  value: unknown,
  recording: string,
): value is Schema<"SourceChoice"> {
  if (
    !object(value) ||
    value.recording_id !== recording ||
    !revision(value.revision) ||
    typeof value.mode !== "string" ||
    !["automatic", "manual", "reviewed"].includes(value.mode) ||
    !Array.isArray(value.sources)
  )
    return false;
  const sources = new Set<string>();
  return value.sources.every((entry: unknown) => {
    if (
      !object(entry) ||
      typeof entry.selected !== "boolean" ||
      typeof entry.selectable !== "boolean" ||
      !object(entry.source)
    )
      return false;
    const source = entry.source;
    if (
      !identity(source.id) ||
      sources.has(source.id) ||
      source.recording_id !== recording ||
      typeof source.label !== "string" ||
      typeof source.provider !== "string" ||
      (source.device_id != null && typeof source.device_id !== "string") ||
      typeof source.ready !== "boolean" ||
      typeof source.can_prepare !== "boolean"
    )
      return false;
    sources.add(source.id);
    return true;
  });
}

function validateOwner(
  account: string,
  recording: string,
  signal: AbortSignal,
) {
  signal.throwIfAborted();
  if (
    typeof account !== "string" ||
    account.length === 0 ||
    !identity(recording)
  )
    throw new Error("Hito could not identify this source choice");
}

export const sourceChoiceApi = {
  async read(
    account: string,
    recording: string,
    signal: AbortSignal,
  ): Promise<Schema<"SourceChoice"> | null> {
    validateOwner(account, recording, signal);
    const started = performance.now();
    const fields = {
      account_id: account,
      recording_id: recording,
      operation_id: createUuid(),
    };
    log.debug("source_choice.read_started", fields);
    try {
      const response = await request(path(recording), {
        action: "source_choice.read",
        account,
        signal,
      });
      const choice: unknown = await response.json();
      signal.throwIfAborted();
      if (response.status !== 200 || !validChoice(choice, recording))
        throw new Error("Hito returned an invalid source choice");
      log.debug("source_choice.read_finished", {
        ...fields,
        mode: choice.mode,
        content_revision: choice.revision,
        count: choice.sources.length,
        duration_ms: Math.round(performance.now() - started),
      });
      return choice;
    } catch (error) {
      if (signal.aborted) {
        log.debug("source_choice.read_finished", {
          ...fields,
          reason: "cancelled",
          duration_ms: Math.round(performance.now() - started),
        });
        signal.throwIfAborted();
      }
      if (error instanceof HttpError && error.status === 404) {
        log.debug("source_choice.read_finished", {
          ...fields,
          mode: "unavailable",
          duration_ms: Math.round(performance.now() - started),
        });
        return null;
      }
      log.warn("source_choice.read_failed", {
        ...fields,
        reason: error instanceof HttpError ? "http" : "request",
        status: error instanceof HttpError ? error.status : undefined,
        duration_ms: Math.round(performance.now() - started),
      });
      throw error;
    }
  },
  async choose(
    account: string,
    recording: string,
    body: Readonly<Schema<"ChooseSource">>,
    signal: AbortSignal,
  ): Promise<Schema<"SourceChoiceReceipt">> {
    validateOwner(account, recording, signal);
    if (!identity(body.request_id) || !identity(body.source_id))
      throw new Error("Hito could not identify this source choice request");
    const response = await request(path(recording), {
      action: "source_choice.choose",
      method: "POST",
      account,
      body,
      signal,
    });
    const receipt: unknown = await response.json();
    signal.throwIfAborted();
    if (
      response.status !== 200 ||
      !object(receipt) ||
      Object.keys(receipt).length !== 4 ||
      receipt.request_id !== body.request_id ||
      receipt.recording_id !== recording ||
      receipt.source_id !== body.source_id ||
      !revision(receipt.revision) ||
      receipt.revision === 0
    )
      throw new Error("Hito returned a different source choice receipt");
    return receipt as Schema<"SourceChoiceReceipt">;
  },
  async reset(
    account: string,
    recording: string,
    body: Readonly<Schema<"ResetSourceChoice">>,
    signal: AbortSignal,
  ): Promise<Schema<"SourceChoiceResetReceipt">> {
    validateOwner(account, recording, signal);
    if (!identity(body.request_id) || !revision(body.expected_revision))
      throw new Error("Hito could not identify this source choice reset");
    const response = await request(`${path(recording)}/reset`, {
      action: "source_choice.reset",
      method: "POST",
      account,
      body,
      signal,
    });
    const receipt: unknown = await response.json();
    signal.throwIfAborted();
    if (
      response.status !== 200 ||
      !object(receipt) ||
      Object.keys(receipt).length !== 3 ||
      receipt.request_id !== body.request_id ||
      receipt.recording_id !== recording ||
      !revision(receipt.revision) ||
      receipt.revision !== body.expected_revision + 1
    )
      throw new Error("Hito returned a different source choice reset receipt");
    return receipt as Schema<"SourceChoiceResetReceipt">;
  },
};
