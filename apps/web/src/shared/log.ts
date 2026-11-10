import type { Schema } from "./schema";

export type LogLevel = "debug" | "info" | "warn" | "error" | "off";
export type LogFields = {
  request_id?: string;
  account_id?: string;
  recording_id?: string;
  source_id?: string;
  playlist_id?: string;
  job_id?: string;
  library_id?: string;
  folder_id?: string;
  operation_id?: string;
  preparation_id?: string;
  method?: string;
  route?: string;
  action?: string;
  page?: string;
  reason?: string;
  mode?: string;
  status?: number;
  content_revision?: number;
  duration_ms?: number;
  media_duration_ms?: number;
  server_duration_ms?: number;
  count?: number;
  index?: number;
  position_ms?: number;
  speed?: number;
  volume?: number;
  enabled?: boolean;
  generation?: number;
  quality?: Schema<"StreamQuality">;
  outcome?: "converted" | "original";
};

type Sink = (
  level: Exclude<LogLevel, "off">,
  event: string,
  fields: LogFields,
) => void;
const priority: Record<LogLevel, number> = {
  debug: 0,
  info: 1,
  warn: 2,
  error: 3,
  off: 4,
};
const permitted = new Set([
  "request_id",
  "account_id",
  "recording_id",
  "source_id",
  "playlist_id",
  "job_id",
  "library_id",
  "folder_id",
  "operation_id",
  "preparation_id",
  "method",
  "route",
  "action",
  "page",
  "reason",
  "mode",
  "status",
  "content_revision",
  "duration_ms",
  "media_duration_ms",
  "server_duration_ms",
  "count",
  "index",
  "position_ms",
  "speed",
  "volume",
  "enabled",
  "generation",
  "quality",
  "outcome",
]);

function permittedValue(key: string, value: unknown) {
  if (key === "generation")
    return (
      typeof value === "number" && Number.isSafeInteger(value) && value >= 0
    );
  if (key === "quality")
    return value === "original" || value === "high" || value === "saver";
  if (key === "outcome") return value === "converted" || value === "original";
  return (
    typeof value === "string" ||
    typeof value === "boolean" ||
    (typeof value === "number" && Number.isFinite(value))
  );
}

export function createLogger(level: LogLevel, sink: Sink) {
  function write(
    severity: Exclude<LogLevel, "off">,
    event: string,
    fields: LogFields = {},
  ) {
    if (priority[severity] < priority[level]) return;
    const safe = Object.fromEntries(
      Object.entries(fields).filter(
        ([key, value]) => permitted.has(key) && permittedValue(key, value),
      ),
    );
    sink(severity, event, safe);
  }
  return {
    debug: (event: string, fields?: LogFields) => write("debug", event, fields),
    info: (event: string, fields?: LogFields) => write("info", event, fields),
    warn: (event: string, fields?: LogFields) => write("warn", event, fields),
    error: (event: string, fields?: LogFields) => write("error", event, fields),
  };
}

function configuredLevel(): LogLevel {
  try {
    const value = localStorage.getItem("hito-log-level");
    if (
      value === "debug" ||
      value === "info" ||
      value === "warn" ||
      value === "error" ||
      value === "off"
    )
      return value;
  } catch {}
  return "info";
}

export const log = createLogger(configuredLevel(), (level, event, fields) => {
  console[level]("[hito]", { event, ...fields });
});
