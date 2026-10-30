import type { Schema } from "../../../shared/schema";

type Connection = Schema<"PluginInstallation">;

const GENERIC = new Set([
  "www",
  "open",
  "music",
  "play",
  "embed",
  "link",
  "app",
  "http",
  "https",
  "com",
  "org",
  "net",
]);
const ALIASES: Record<string, string> = { youtu: "youtube" };

function hostLabels(reference: string): string[] {
  const text = reference.trim().toLowerCase();
  const scheme = /^([a-z][a-z0-9+.-]*):(?!\/\/)/.exec(text);
  if (scheme) return [scheme[1]];
  let host = "";
  try {
    host = new URL(
      /^[a-z][a-z0-9+.-]*:\/\//.test(text) ? text : `https://${text}`,
    ).hostname;
  } catch {
    return [];
  }
  if (!host.includes(".")) return [];
  return host.split(".");
}

function keywords(reference: string): string[] {
  return hostLabels(reference)
    .filter((label) => label.length >= 4 && !GENERIC.has(label))
    .map((label) => ALIASES[label] ?? label);
}

export type InferredService = {
  provider: Connection | null;
  /** The pasted text names one of the connected services. */
  recognised: boolean;
  /** Several services are connected and the text does not choose one. */
  ambiguous: boolean;
};

/** Chooses the playlist service from a pasted link or identifier. */
export function inferPlaylistService(
  reference: string,
  providers: Connection[],
  names: Record<string, string>,
): InferredService {
  const words = keywords(reference);
  const matches = providers.filter((provider) => {
    const haystack = [
      provider.manifest.id,
      provider.manifest.name,
      names[provider.service_id] ?? "",
    ]
      .join(" ")
      .toLowerCase();
    return words.some((word) => haystack.includes(word));
  });
  if (matches.length === 1)
    return { provider: matches[0], recognised: true, ambiguous: false };
  if (providers.length === 1 && matches.length === 0)
    return { provider: providers[0], recognised: false, ambiguous: false };
  return { provider: null, recognised: false, ambiguous: providers.length > 1 };
}
