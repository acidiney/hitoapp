import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { creditMemberApi } from "../api/creditMemberApi";
import { artistIdentityApi } from "../api/artistIdentityApi";
import { HttpError } from "../../../shared/http";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { metadataPreferencesApi } from "../../metadata/api/metadataPreferencesApi";
import { pluginsApi } from "../../plugins/api/pluginsApi";
import {
  providerKey,
  savedProviders,
  unavailable,
  type ArtistProvider,
} from "./artistReviewRules";

type Choice = Schema<"RecordingCreditMemberIdentityChoice">;
type Context = Schema<"CreditMemberReviewContext">;
type Found = Extract<
  Schema<"CreditMemberCandidateSearchAvailability">,
  { status: "available" }
>;
type Preview = Extract<
  Schema<"CreditMemberCandidatePreviewAvailability">,
  { status: "available" }
>;
type Selection = Schema<"ArtistIdentitySelection">;
type Receipt = Schema<"CreditMemberIdentityChoiceReceipt">;
type Phase = "read" | "load" | "search" | "preview" | "choose" | "reset";
type ResetAttempt = { requestId: string; revision: number };
type Owner = {
  generation: number;
  controller: AbortController | null;
  phase: Phase | null;
  attempt: ResetAttempt | null;
};
type State = {
  owner: Owner;
  choice: Choice | null;
  name: string | null;
  loaded: boolean;
  enabled: boolean;
  providers: ArtistProvider[];
  selected: string;
  query: string;
  results: Found | null;
  preview: Preview | null;
  pending: Phase | null;
  error: string;
  failed: Phase | null;
  unconfirmed: boolean;
};

function empty(owner: Owner, query: string): State {
  return {
    owner,
    choice: null,
    name: null,
    loaded: false,
    enabled: false,
    providers: [],
    selected: "",
    query,
    results: null,
    preview: null,
    pending: null,
    error: "",
    failed: null,
    unconfirmed: false,
  };
}

function rejected(error: unknown) {
  return (
    error instanceof HttpError && error.status >= 400 && error.status < 500
  );
}

function sameSelection(left: Selection, right: Selection) {
  return (
    left.group_id === right.group_id &&
    left.association_id === right.association_id
  );
}

function failure(error: unknown) {
  if (error instanceof HttpError && error.status === 409)
    return "This artist changed. Refresh and try again.";
  if (error instanceof HttpError && error.status === 404)
    return "This credited artist can no longer be reviewed.";
  return errorMessage(error);
}

export function useCreditMemberChoice(
  account: string,
  detail: Schema<"RecordingMetadata">,
  member: Schema<"CreditedArtistView"> & { artist_id: string },
) {
  const recording = detail.recording_id;
  const artist = member.artist_id;
  const scope = useRequestScope(
    JSON.stringify([
      account,
      recording,
      artist,
      member.name,
      member.role,
      detail.selected_observation?.id,
    ]),
  );
  const [owner] = useState<Owner>(() => ({
    generation: 0,
    controller: null,
    phase: null,
    attempt: null,
  }));
  const [snapshot, setSnapshot] = useState(() => empty(owner, member.name));
  const current =
    snapshot.owner === owner ? snapshot : empty(owner, member.name);
  const provider =
    current.providers.find((item) => item.key === current.selected) ?? null;

  function requireContext(context: Context, selected: ArtistProvider) {
    if (
      context.account_id !== account ||
      context.recording_id !== recording ||
      context.artist_id !== artist ||
      context.credited_name !== member.name ||
      context.role !== member.role ||
      providerKey(context.provider) !== selected.key
    )
      throw new Error(
        "The artist review did not belong to this credited artist and provider. Search again.",
      );
  }

  function requireChoice(choice: Choice) {
    if (
      choice.recording_id !== recording ||
      choice.artist_id !== artist ||
      choice.role !== member.role ||
      !Number.isSafeInteger(choice.revision) ||
      choice.revision < 0
    )
      throw new Error("Invalid credited artist response");
    return choice;
  }

  function requireReceipt(
    receipt: Receipt,
    requestId: string,
    previous: number,
  ) {
    if (
      receipt.request_id !== requestId ||
      receipt.recording_id !== recording ||
      receipt.artist_id !== artist ||
      receipt.role !== member.role ||
      !Number.isSafeInteger(receipt.revision) ||
      receipt.revision <= previous
    )
      throw new Error("Invalid credited artist receipt");
    return receipt;
  }

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
    if (!request.active() || owner.phase === "choose") return null;
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
      ...(value.owner === owner ? value : empty(owner, member.name)),
      ...initial,
      owner,
      pending: phase,
      error: "",
      failed: null,
    }));
    log.debug("credit_member_choice.started", {
      action: phase,
      request_id: requestId,
    });
    try {
      const result = await load(signal);
      if (!active()) return null;
      const accepted = publish(result);
      setSnapshot((value) =>
        value.owner === owner ? { ...value, ...accepted } : value,
      );
      outcome = "accepted";
      return result;
    } catch (error) {
      if (active()) {
        outcome = "failed";
        setSnapshot((value) =>
          value.owner === owner
            ? {
                ...value,
                error: failure(error),
                failed: phase,
                results:
                  phase === "choose" || phase === "reset"
                    ? value.results
                    : null,
                preview:
                  phase === "choose" || phase === "reset"
                    ? value.preview
                    : null,
              }
            : value,
        );
      }
      return null;
    } finally {
      if (owner.controller === controller) {
        owner.phase = null;
        owner.controller = null;
      }
      if (active())
        setSnapshot((value) =>
          value.owner === owner ? { ...value, pending: null } : value,
        );
      const record = outcome === "failed" ? log.warn : log.debug;
      record("credit_member_choice.finished", {
        action: phase,
        request_id: requestId,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }

  async function readChoice(
    signal: AbortSignal,
    known: { name: string; selection: Selection } | null,
  ) {
    const choice = requireChoice(
      await creditMemberApi.choice(account, recording, artist, signal),
    );
    let name: string | null = null;
    if (
      choice.state.status !== "unselected" &&
      known &&
      sameSelection(choice.state, known.selection)
    )
      name = known.name;
    else if (choice.state.status !== "unselected") {
      try {
        const group = await artistIdentityApi.group(
          account,
          choice.state.group_id,
          signal,
        );
        name = group.group.name.value;
      } catch {
        signal.throwIfAborted();
      }
    }
    return { choice, name };
  }

  async function read() {
    if (current.choice || owner.phase) return;
    await run(
      "read",
      {},
      (signal) => readChoice(signal, null),
      ({ choice, name }) => ({ choice, name }),
    );
  }

  async function refresh() {
    if (!current.unconfirmed || owner.phase) return;
    await run(
      "read",
      {},
      (signal) => readChoice(signal, null),
      ({ choice, name }) => ({ choice, name, unconfirmed: false }),
    );
  }

  async function reset() {
    const choice = current.choice;
    if (!choice || choice.state.status === "unselected" || owner.phase)
      return false;
    const attempt = owner.attempt ?? {
      requestId: createUuid(),
      revision: choice.revision,
    };
    owner.attempt = attempt;
    const done = await run(
      "reset",
      {},
      async (signal) => {
        let receipt: Receipt;
        try {
          receipt = await creditMemberApi.reset(
            account,
            recording,
            artist,
            {
              request_id: attempt.requestId,
              expected_revision: attempt.revision,
            },
            signal,
          );
        } catch (error) {
          if (rejected(error)) {
            owner.attempt = null;
            const fresh = await readChoice(signal, null).catch(() => null);
            if (fresh)
              setSnapshot((value) =>
                value.owner === owner ? { ...value, ...fresh } : value,
              );
          }
          throw error;
        }
        requireReceipt(receipt, attempt.requestId, attempt.revision);
        if (receipt.selection)
          throw new Error("Invalid credited artist receipt");
        owner.attempt = null;
        return readChoice(signal, null);
      },
      ({ choice, name }) => ({ choice, name }),
    );
    return done !== null;
  }

  function dismiss() {
    if (owner.phase === "choose") return;
    const running =
      owner.phase === "load" ||
      owner.phase === "search" ||
      owner.phase === "preview";
    if (running) cancel();
    setSnapshot((value) => {
      if (value.owner !== owner) return value;
      const stale =
        running ||
        value.failed === "load" ||
        value.failed === "search" ||
        value.failed === "preview";
      return {
        ...value,
        query: member.name,
        results: null,
        preview: null,
        pending: running ? null : value.pending,
        error: stale ? "" : value.error,
        failed: stale ? null : value.failed,
      };
    });
    void refresh();
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
        ]),
      ([settings, plugins]) => {
        const providers = savedProviders(settings, plugins);
        return {
          loaded: true,
          enabled: settings.enabled,
          providers,
          selected: providers[0]?.key ?? "",
        };
      },
    );
  }

  function edit(change: Partial<State>) {
    if (!scope.capture().active() || owner.phase === "choose") return;
    cancel();
    setSnapshot((value) =>
      value.owner === owner
        ? {
            ...value,
            ...change,
            results: null,
            preview: null,
            pending: null,
            error: "",
          }
        : value,
    );
  }

  async function search() {
    const query = current.query.trim();
    if (!provider?.available || !current.enabled || !query) return;
    await run(
      "search",
      { results: null, preview: null },
      async (signal) => {
        const context = await creditMemberApi.context(
          account,
          recording,
          artist,
          { provider: provider.value },
          signal,
        );
        requireContext(context, provider);
        const result = await creditMemberApi.search(
          account,
          recording,
          artist,
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

  async function previewCandidate(candidate: Schema<"ArtistSearchCandidate">) {
    const results = current.results;
    if (
      !provider?.available ||
      !results ||
      !results.page.items.some((item) => item.artist.id === candidate.artist.id)
    )
      return;
    await run(
      "preview",
      { preview: null },
      async (signal) => {
        const result = await creditMemberApi.preview(
          account,
          recording,
          artist,
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

  async function choose() {
    const preview = current.preview;
    const choice = current.choice;
    if (!preview || !choice || !provider?.available) return false;
    const done = await run(
      "choose",
      {},
      async (signal) => {
        const confirmed = await creditMemberApi.confirm(
          account,
          recording,
          artist,
          {
            context: preview.context,
            provider_artist_id: preview.profile.artist.id,
          },
          signal,
        );
        if (
          confirmed.account_id !== account ||
          confirmed.recording_id !== recording ||
          confirmed.artist_id !== artist ||
          confirmed.library_id !== preview.context.library_id ||
          confirmed.observation_id !== preview.context.observation_id ||
          providerKey(confirmed.provider) !== provider.key ||
          confirmed.provider_artist_id !== preview.profile.artist.id
        )
          throw new Error(
            "The artist confirmation did not belong to this credited artist and provider.",
          );
        const requestId = createUuid();
        owner.attempt = null;
        const receipt = await creditMemberApi.choose(
          account,
          recording,
          artist,
          {
            request_id: requestId,
            expected_revision: choice.revision,
            association_id: confirmed.id,
            context: preview.context,
          },
          signal,
        );
        setSnapshot((value) =>
          value.owner === owner ? { ...value, unconfirmed: true } : value,
        );
        requireReceipt(receipt, requestId, choice.revision);
        const selection = receipt.selection;
        if (!selection) throw new Error("Invalid credited artist receipt");
        const readback = await readChoice(signal, {
          name: preview.profile.artist.name,
          selection,
        });
        if (readback.choice.revision < receipt.revision)
          throw new Error("Invalid credited artist response");
        return readback;
      },
      ({ choice, name }) => ({
        choice,
        name,
        preview: null,
        results: null,
        unconfirmed: false,
      }),
    );
    return done !== null;
  }

  return {
    ...current,
    provider,
    read,
    refresh,
    reset,
    dismiss,
    open,
    search,
    previewCandidate,
    choose,
    select: (key: string) => {
      if (current.providers.some((item) => item.key === key && item.available))
        edit({ selected: key });
    },
    setQuery: (query: string) => edit({ query }),
  };
}
