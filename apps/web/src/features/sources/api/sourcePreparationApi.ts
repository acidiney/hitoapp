import type { Schema } from "../../../shared/schema";
import { HttpError, request, requestJson } from "../../../shared/http";
import { sourceStatusStore } from "../model/sourceStatusStore";

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const identity = (value: unknown): value is string =>
  typeof value === "string" &&
  uuid.test(value) &&
  value !== "00000000-0000-0000-0000-000000000000";
const phases = new Set<Schema<"SourcePreparationPhase">>([
  "queued",
  "submitting",
  "preparing",
  "ready",
  "blocked",
  "failed",
  "submission_unknown",
  "cancelled",
]);
const reasons = new Set<Schema<"PreparationReason">>([
  "authorization_required",
  "quota_reached",
  "rate_limited",
  "provider_restricted",
  "source_unavailable",
  "source_changed",
  "unsupported",
  "provider_unavailable",
  "invalid_response",
]);
const integer = (value: unknown, maximum = Number.MAX_SAFE_INTEGER) =>
  typeof value === "number" &&
  Number.isSafeInteger(value) &&
  value >= 0 &&
  value <= maximum;

function validStatus(
  value: Schema<"SourcePreparationStatus">,
  recording: string,
  source: string,
) {
  return (
    value?.recording_id === recording &&
    value.source_id === source &&
    identity(value.preparation_id) &&
    phases.has(value.phase) &&
    (value.progress_basis_points == null ||
      integer(value.progress_basis_points, 10_000)) &&
    (value.eta_seconds == null ||
      integer(value.eta_seconds, 7 * 24 * 60 * 60)) &&
    (value.reason == null || reasons.has(value.reason)) &&
    (value.next_check_at == null || integer(value.next_check_at)) &&
    integer(value.created_at) &&
    integer(value.updated_at)
  );
}

const path = (recording: string, source: string) =>
  `/recordings/${encodeURIComponent(recording)}/sources/${encodeURIComponent(source)}/preparation`;

export const sourcePreparationApi = {
  async create(
    account: string,
    recording: string,
    source: string,
    body: Readonly<Schema<"PrepareSource">>,
    signal: AbortSignal,
  ): Promise<Schema<"SourcePreparationReceipt">> {
    const response = await request(path(recording, source), {
      action: "source_preparation.create",
      method: "POST",
      account,
      body,
      signal,
    });
    if (response.status !== 202)
      throw new Error("Hito did not confirm source preparation");
    const receipt: Schema<"SourcePreparationReceipt"> = await response.json();
    if (
      receipt?.request_id !== body.request_id ||
      receipt.recording_id !== recording ||
      receipt.source_id !== source ||
      !identity(receipt.preparation_id)
    )
      throw new Error("Hito returned a different preparation receipt");
    sourceStatusStore.invalidate(account, recording);
    return receipt;
  },
  async status(
    account: string,
    recording: string,
    source: string,
    signal: AbortSignal,
  ): Promise<Schema<"SourcePreparationStatus"> | null> {
    try {
      const status: Schema<"SourcePreparationStatus"> = await requestJson<
        Schema<"SourcePreparationStatus">
      >(path(recording, source), {
        action: "source_preparation.status",
        account,
        signal,
      });
      if (!validStatus(status, recording, source))
        throw new Error("Hito returned an invalid preparation status");
      return status;
    } catch (error) {
      if (error instanceof HttpError && error.status === 404) return null;
      throw error;
    }
  },
};
