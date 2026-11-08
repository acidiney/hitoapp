import type { Schema } from "../../../shared/schema";

export type SourceTone = "action" | "success" | "warning";

export function sourceLabels(source: Schema<"Source">) {
  if (source.provider === "local")
    return { primary: source.label, secondary: "Music folder" };
  const service = source.provider_label?.trim();
  if (service)
    return {
      primary: service,
      secondary: source.label.replaceAll(" / ", " · "),
    };
  return { primary: source.label, secondary: "Music service" };
}

export function sourceState(source: Schema<"Source">): {
  tone: SourceTone;
  label: string;
} {
  if (source.device_id) return { tone: "action", label: "On this device" };
  return source.ready
    ? { tone: "success", label: "Ready" }
    : { tone: "warning", label: "Not prepared" };
}

export function bestGuessReason(source: Schema<"Source">) {
  const provider = source.provider_label?.trim() || "the music service";
  return `Chosen from ${provider}'s top result because no exact match was found.`;
}
