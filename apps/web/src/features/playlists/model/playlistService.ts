import type { Schema } from "../../../shared/schema";

type Connection = Schema<"PluginInstallation">;

const HOSTS: Record<string, string> = {
  "open.spotify.com": "spotify",
  "play.spotify.com": "spotify",
  "music.youtube.com": "youtube",
  "www.youtube.com": "youtube",
  "youtube.com": "youtube",
  "m.youtube.com": "youtube",
  "youtu.be": "youtube",
  "jamendo.com": "jamendo",
  "www.jamendo.com": "jamendo",
};
const SCHEMES: Record<string, string> = { spotify: "spotify" };

function serviceKeyword(reference: string): string | null {
  const text = reference.trim().toLowerCase();
  const scheme = /^([a-z][a-z0-9+.-]*):(?!\/\/)/.exec(text);
  if (scheme) return SCHEMES[scheme[1]] ?? null;
  try {
    const url = new URL(
      /^[a-z][a-z0-9+.-]*:\/\//.test(text) ? text : `https://${text}`,
    );
    return HOSTS[url.hostname] ?? null;
  } catch {
    return null;
  }
}

export type InferredService = {
  provider: Connection | null;
  recognised: boolean;
  ambiguous: boolean;
};

export function inferPlaylistService(
  reference: string,
  providers: Connection[],
  names: Record<string, string>,
): InferredService {
  const keyword = serviceKeyword(reference);
  const matches = providers.filter((provider) => {
    const haystack = [
      provider.manifest.id,
      provider.manifest.name,
      names[provider.service_id] ?? "",
    ]
      .join(" ")
      .toLowerCase();
    return keyword !== null && haystack.includes(keyword);
  });
  if (matches.length === 1)
    return { provider: matches[0], recognised: true, ambiguous: false };
  if (providers.length === 1 && matches.length === 0)
    return { provider: providers[0], recognised: false, ambiguous: false };
  return { provider: null, recognised: false, ambiguous: providers.length > 1 };
}
