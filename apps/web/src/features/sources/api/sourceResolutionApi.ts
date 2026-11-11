import type { Schema } from "../../../shared/schema";
import { request } from "../../../shared/http";
import { log } from "../../../shared/log";
import { invalidate } from "../../../shared/invalidation";

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const identity = (value: unknown): value is string =>
  typeof value === "string" &&
  uuid.test(value) &&
  value !== "00000000-0000-0000-0000-000000000000";
const outcomes = new Set<Schema<"SourceResolution">["outcome"]>([
  "ready",
  "no_suitable_source",
  "provider_unavailable",
  "protected_choice",
  "cancelled",
]);

function validSource(source: Schema<"Source">, recording: string) {
  return (
    source !== null &&
    typeof source === "object" &&
    identity(source.id) &&
    source.recording_id === recording &&
    typeof source.provider === "string" &&
    source.provider.length > 0 &&
    typeof source.label === "string" &&
    source.label.length > 0 &&
    typeof source.ready === "boolean" &&
    typeof source.can_prepare === "boolean" &&
    (source.device_id == null || typeof source.device_id === "string")
  );
}

export const sourceResolutionApi = {
  async resolve(
    account: string,
    recording: string,
    body: Readonly<Schema<"ResolveSource">>,
    signal: AbortSignal,
  ): Promise<Schema<"SourceResolution">> {
    const started = performance.now();
    const response = await request(
      `/recordings/${encodeURIComponent(recording)}/sources/resolve`,
      {
        action: "source_resolution.resolve",
        method: "POST",
        account,
        body,
        signal,
      },
    );
    if (response.status !== 200)
      throw new Error("Hito did not confirm source resolution");
    const resolution: Schema<"SourceResolution"> = await response.json();
    signal.throwIfAborted();
    if (
      resolution?.request_id !== body.request_id ||
      resolution.recording_id !== recording ||
      !identity(resolution.resolution_id) ||
      !outcomes.has(resolution.outcome) ||
      !Array.isArray(resolution.sources) ||
      !resolution.sources.every((source) => validSource(source, recording))
    )
      throw new Error("Hito returned an invalid source resolution");
    log.info("playback.source_resolution_completed", {
      account_id: account,
      recording_id: recording,
      request_id: body.request_id,
      operation_id: resolution.resolution_id,
      reason: resolution.outcome,
      count: resolution.sources.length,
      duration_ms: Math.round(performance.now() - started),
    });
    if (resolution.sources.length > 0) invalidate("sources", recording);
    return resolution;
  },
};
