import { useMemo, useState } from "react";
import { artistReviewApi } from "../api/artistReviewApi";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";
import { metadataPreferencesApi } from "../../metadata/api/metadataPreferencesApi";
import { pluginsApi } from "../../plugins/api/pluginsApi";
import type {
  ArtistAssociation,
  ArtistCandidatePreviewAvailability,
  ArtistCandidateSearchAvailability,
  ArtistProfileAvailability,
  ArtistReviewContext,
  ArtistSearchCandidate,
  MetadataProvider,
  RecordingMetadata,
} from "../../../shared/types";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

type Found = Extract<
  ArtistCandidateSearchAvailability,
  { status: "available" }
>;
type Preview = Extract<
  ArtistCandidatePreviewAvailability,
  { status: "available" }
>;
type Phase = "load" | "search" | "preview" | "confirm" | "profile" | "refresh";
type Owner = {
  generation: number;
  controller: AbortController | null;
  phase: Phase | null;
};
type Provider = {
  key: string;
  label: string;
  value: MetadataProvider;
  available: boolean;
};
type State = {
  owner: Owner;
  loaded: boolean;
  enabled: boolean;
  providers: Provider[];
  selected: string;
  query: string;
  results: Found | null;
  preview: Preview | null;
  associations: ArtistAssociation[];
  profile: ArtistProfileAvailability | null;
  saved: boolean;
  pending: Phase | null;
  error: string;
};

function empty(owner: Owner, query: string): State {
  return {
    owner,
    query,
    loaded: false,
    enabled: false,
    providers: [],
    selected: "",
    results: null,
    preview: null,
    associations: [],
    profile: null,
    saved: false,
    pending: null,
    error: "",
  };
}

function providerKey(provider: MetadataProvider) {
  return provider.kind === "builtin_musicbrainz"
    ? provider.kind
    : provider.installation_id;
}

function unavailable(
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

export function useArtistReview(account: string, detail: RecordingMetadata) {
  const recording = detail.recording_id;
  const scope = useRequestScope(
    JSON.stringify([
      account,
      recording,
      detail.selected_observation?.id,
      detail.artist.operation_id,
      detail.artist.value,
    ]),
  );
  const owner = useMemo<Owner>(
    () => ({ generation: 0, controller: null, phase: null }),
    [scope],
  );
  const [snapshot, setSnapshot] = useState(() =>
    empty(owner, detail.artist.value),
  );
  const current =
    snapshot.owner === owner ? snapshot : empty(owner, detail.artist.value);
  const provider =
    current.providers.find((item) => item.key === current.selected) ?? null;
  const writing = () => owner.phase === "confirm" || owner.phase === "profile";

  function cancel() {
    owner.controller?.abort();
    owner.controller = null;
    owner.phase = null;
    owner.generation++;
  }

  async function run<T>(
    phase: Phase,
    initial: Partial<State>,
    load: (signal: AbortSignal) => Promise<T>,
    publish: (result: T) => Partial<State>,
  ): Promise<T | null> {
    const request = scope.capture();
    if (!request.active() || writing()) return null;
    cancel();
    const controller = new AbortController();
    owner.controller = controller;
    owner.phase = phase;
    const generation = owner.generation;
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const active = () =>
      request.active() && generation === owner.generation && !signal.aborted;
    const requestId = createUuid();
    const started = performance.now();
    let outcome = "cancelled";
    setSnapshot((value) => ({
      ...(value.owner === owner ? value : empty(owner, detail.artist.value)),
      ...initial,
      owner,
      pending: phase,
      error: "",
    }));
    log.debug("artist_review.started", {
      action: phase,
      request_id: requestId,
    });
    try {
      const result = await load(signal);
      if (!active()) return null;
      const accepted = publish(result);
      setSnapshot((value) =>
        request.active() && value.owner === owner
          ? { ...value, ...accepted }
          : value,
      );
      outcome = "accepted";
      return result;
    } catch (error) {
      if (active()) {
        outcome = "failed";
        setSnapshot((value) =>
          active() && value.owner === owner
            ? {
                ...value,
                error: errorMessage(error),
                results: null,
                preview: null,
              }
            : value,
        );
      }
      return null;
    } finally {
      if (owner.controller === controller) owner.phase = null;
      if (active())
        setSnapshot((value) =>
          active() && value.owner === owner
            ? { ...value, pending: null }
            : value,
        );
      const record =
        outcome === "failed"
          ? log.warn
          : phase === "confirm" && outcome === "accepted"
            ? log.info
            : log.debug;
      record("artist_review.finished", {
        action: phase,
        request_id: requestId,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  function requireContext(context: ArtistReviewContext, selected: Provider) {
    if (
      context.account_id !== account ||
      context.recording_id !== recording ||
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

  async function open() {
    if (current.loaded || owner.phase) return;
    await run(
      "load",
      {},
      (signal) =>
        Promise.all([
          metadataPreferencesApi.settings(account, signal),
          pluginsApi.settings(account, signal),
          artistReviewApi.associations(account, recording, signal),
        ]),
      ([settings, plugins, associations]) => {
        const providers = settings.providers.map((value): Provider => {
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
        return {
          loaded: true,
          enabled: settings.enabled,
          providers,
          associations,
          selected: providers[0]?.key ?? "",
        };
      },
    );
  }

  function edit(change: Partial<State>) {
    if (!scope.capture().active() || writing()) return;
    cancel();
    setSnapshot((value) =>
      value.owner === owner
        ? {
            ...value,
            ...change,
            results: null,
            preview: null,
            profile: null,
            saved: false,
            pending: null,
            error: "",
          }
        : value,
    );
  }

  async function search() {
    const query = current.query.trim();
    if (!provider?.available || !current.enabled || !query || writing()) return;
    await run(
      "search",
      { results: null, preview: null, profile: null, saved: false },
      async (signal) => {
        const context = await artistReviewApi.context(
          account,
          recording,
          { provider: provider.value },
          signal,
        );
        requireContext(context, provider);
        const result = await artistReviewApi.search(
          account,
          recording,
          {
            context,
            search: { query, limit: 20, cursor: null, snapshot: null },
          },
          signal,
        );
        if (result.status !== "available")
          throw new Error(unavailable(result.status));
        requireContext(result.context, provider);
        return result;
      },
      (results) => ({ results }),
    );
  }

  async function previewCandidate(candidate: ArtistSearchCandidate) {
    const results = current.results;
    if (
      !provider?.available ||
      !results ||
      owner.phase ||
      !results.page.items.some((item) => item.artist.id === candidate.artist.id)
    )
      return;
    await run(
      "preview",
      { preview: null, profile: null, saved: false },
      async (signal) => {
        const result = await artistReviewApi.preview(
          account,
          recording,
          {
            context: results.context,
            provider_artist_id: candidate.artist.id,
          },
          signal,
        );
        if (result.status !== "available")
          throw new Error(unavailable(result.status));
        requireContext(result.context, provider);
        if (result.profile.artist.id !== candidate.artist.id)
          throw new Error(
            "The provider returned another artist. Search again.",
          );
        return result;
      },
      (preview) => ({ preview }),
    );
  }

  async function loadProfile(association: ArtistAssociation) {
    if (
      !scope.capture().active() ||
      owner.phase ||
      association.account_id !== account ||
      association.recording_id !== recording
    )
      return;
    await run(
      "profile",
      { profile: null },
      async (signal) => {
        const profile = await artistReviewApi.profile(
          account,
          recording,
          association.id,
          signal,
        );
        if (
          profile.status === "available" &&
          (profile.association_id !== association.id ||
            profile.profile.artist.id !== association.provider_artist_id)
        )
          throw new Error(
            "The reviewed artist profile did not belong to this saved review.",
          );
        return profile;
      },
      (profile) => ({ profile }),
    );
  }

  async function refreshAssociations() {
    const request = scope.capture();
    if (!request.active()) return;
    const associations = await artistReviewApi.associations(
      account,
      recording,
      request.signal,
    );
    if (!request.active()) return;
    setSnapshot((value) => {
      if (value.owner !== owner) return value;
      const shown = value.profile;
      const removed =
        shown?.status === "available" &&
        !associations.some((item) => item.id === shown.association_id);
      return { ...value, associations, profile: removed ? null : shown };
    });
  }

  async function confirm() {
    const request = scope.capture();
    const preview = current.preview;
    if (!request.active() || !preview || !provider?.available || owner.phase)
      return;
    const association = await run(
      "confirm",
      {},
      async (signal) => {
        const result = await artistReviewApi.confirm(
          account,
          recording,
          {
            context: preview.context,
            provider_artist_id: preview.profile.artist.id,
          },
          signal,
        );
        if (
          result.account_id !== account ||
          result.recording_id !== recording ||
          result.artist_id !== preview.context.artist_id ||
          result.library_id !== preview.context.library_id ||
          result.observation_id !== preview.context.observation_id ||
          result.source_id !== preview.context.source_id ||
          providerKey(result.provider) !== provider.key ||
          result.provider_artist_id !== preview.profile.artist.id
        )
          throw new Error(
            "The artist confirmation did not belong to this track and provider.",
          );
        return result;
      },
      (association) => ({
        saved: true,
        preview: null,
        results: null,
        associations: [
          ...current.associations.filter((item) => item.id !== association.id),
          association,
        ],
      }),
    );
    if (association && request.active()) await loadProfile(association);
  }

  return {
    ...current,
    provider,
    open,
    search,
    previewCandidate,
    confirm,
    loadProfile,
    refreshAssociations,
    choose: (key: string) => {
      if (current.providers.some((item) => item.key === key && item.available))
        edit({ selected: key });
    },
    setQuery: (query: string) => edit({ query }),
  };
}
