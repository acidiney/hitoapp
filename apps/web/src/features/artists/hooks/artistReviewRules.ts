import type { Schema } from "../../../shared/schema";

export type ArtistProvider = {
  key: string;
  label: string;
  value: Schema<"MetadataProvider">;
  available: boolean;
};

export function providerKey(provider: Schema<"MetadataProvider">) {
  return provider.kind === "builtin_musicbrainz"
    ? provider.kind
    : provider.installation_id;
}

export function unavailable(
  status: "disabled" | "unsupported" | "unavailable" | "cancelled",
) {
  switch (status) {
    case "disabled":
      return "Metadata providers are disabled in your settings.";
    case "unsupported":
      return "This provider does not support this artist information request.";
    case "unavailable":
      return "Artist information is unavailable. Try again.";
    case "cancelled":
      return "The artist information request was cancelled.";
  }
}

export function savedProviders(
  settings: Schema<"MetadataSettings">,
  plugins: Schema<"PluginSettings">,
) {
  return settings.providers.map((value): ArtistProvider => {
    if (value.kind === "builtin_musicbrainz")
      return {
        key: providerKey(value),
        label: "MusicBrainz",
        value,
        available: true,
      };
    const installation = plugins.installations.find(
      (item) =>
        item.id === value.installation_id &&
        item.manifest.capabilities.includes("catalog"),
    );
    const service = plugins.services.find(
      (item) => item.id === installation?.service_id,
    );
    return {
      key: providerKey(value),
      label: service?.name ?? "Unavailable saved provider",
      value,
      available: !!installation && !!service,
    };
  });
}

export function requireReviewContext(
  context: Schema<"ArtistReviewContext">,
  selected: ArtistProvider,
  account: string,
  detail: Schema<"RecordingMetadata">,
) {
  if (
    context.account_id !== account ||
    context.recording_id !== detail.recording_id ||
    providerKey(context.provider) !== selected.key
  )
    throw new Error(
      "The artist review did not belong to this track and provider. Search again.",
    );
  if (
    context.credited_name !== detail.artist.value ||
    context.artist_operation_id !== (detail.artist.operation_id ?? null) ||
    context.observation_id !== detail.selected_observation?.id ||
    context.source_id !== (detail.selected_observation?.source_id ?? null)
  )
    throw new Error(
      "The saved artist credit changed. Refresh this track before reviewing artist information.",
    );
}

export function requireConfirmation(
  result: Schema<"ArtistAssociation">,
  account: string,
  recording: string,
  preview: {
    context: Schema<"ArtistReviewContext">;
    profile: { artist: { id: string } };
  },
  selected: ArtistProvider,
) {
  if (
    result.account_id !== account ||
    result.recording_id !== recording ||
    result.artist_id !== preview.context.artist_id ||
    result.library_id !== preview.context.library_id ||
    result.observation_id !== preview.context.observation_id ||
    result.source_id !== preview.context.source_id ||
    providerKey(result.provider) !== selected.key ||
    result.provider_artist_id !== preview.profile.artist.id
  )
    throw new Error(
      "The artist confirmation did not belong to this track and provider.",
    );
}

export function requireSavedProfile(
  profile: Schema<"ArtistProfileAvailability">,
  association: Schema<"ArtistAssociation">,
) {
  if (
    profile.status === "available" &&
    (profile.association_id !== association.id ||
      profile.profile.artist.id !== association.provider_artist_id)
  )
    throw new Error(
      "The reviewed artist profile did not belong to this saved review.",
    );
}
