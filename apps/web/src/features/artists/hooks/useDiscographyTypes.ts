import type { Schema } from "../../../shared/schema";
import { useMemo, useState } from "react";
import { artistDiscographyApi } from "../api/artistDiscographyApi";
import { errorMessage } from "../../../shared/format";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { discographyUnavailable } from "./useArtistDiscography";
import { DeadlineError, withDeadline } from "./withDeadline";

const PAGE_LIMIT = 20;
const MAX_PAGES_PER_TYPE = 50;
const REQUEST_DEADLINE_MS = 20_000;

export type ReleaseType = NonNullable<
  Schema<"ReadArtistDiscography">["release_type"]
>;

export type TypePaging = {
  groups: Schema<"CatalogReleaseGroup">[];
  continuation: string | null;
  started: boolean;
  pages: number;
  error: string;
  restartRequired: boolean;
};

type Owner = { pending: boolean };
type State = {
  owner: Owner;
  unsupported: boolean;
  pending: ReleaseType | null;
  types: Partial<Record<ReleaseType, TypePaging>>;
};

const empty = (owner: Owner): State => ({
  owner,
  unsupported: false,
  pending: null,
  types: {},
});

const fresh: TypePaging = {
  groups: [],
  continuation: null,
  started: false,
  pages: 0,
  error: "",
  restartRequired: false,
};

function belongs(
  type: ReleaseType,
  kind: Schema<"CatalogReleaseGroup">["kind"],
) {
  const named = kind === "album" || kind === "ep" || kind === "single";
  return type === "other" ? !named : kind === type;
}

/** Pages one release type at a time, from providers that can, beside the whole-catalog read. */
export function useDiscographyTypes(
  account: string,
  recording: string,
  association: Schema<"ArtistAssociation">,
  profile: Schema<"CatalogArtistProfile">,
  enabled: boolean,
) {
  const scope = useRequestScope(
    JSON.stringify([account, recording, association, profile]),
  );
  const owner = useMemo<Owner>(() => ({ pending: false }), [scope]);
  const [snapshot, setSnapshot] = useState(() => empty(owner));
  const current = snapshot.owner === owner ? snapshot : empty(owner);

  function update(
    type: ReleaseType,
    change: (value: TypePaging) => TypePaging,
    extra: Partial<State> = {},
  ) {
    setSnapshot((value) => {
      const base = value.owner === owner ? value : empty(owner);
      return {
        ...base,
        ...extra,
        types: {
          ...base.types,
          [type]: change(base.types[type] ?? fresh),
        },
      };
    });
  }

  async function more(type: ReleaseType) {
    const request = scope.capture();
    const known = current.types[type] ?? fresh;
    if (
      !enabled ||
      !request.active() ||
      owner.pending ||
      current.unsupported ||
      known.pages >= MAX_PAGES_PER_TYPE ||
      (known.pages > 0 &&
        known.continuation === null &&
        !known.restartRequired) ||
      association.account_id !== account ||
      association.recording_id !== recording ||
      association.provider_artist_id !== profile.artist.id
    )
      return;
    owner.pending = true;
    const requestId = createUuid();
    const started = performance.now();
    const restarting = known.restartRequired;
    let outcome = "cancelled";
    let count = 0;
    setSnapshot((value) => ({
      ...(value.owner === owner ? value : empty(owner)),
      pending: type,
    }));
    update(type, (value) => ({ ...value, started: true, error: "" }));
    log.debug("artist_discography_type.started", {
      request_id: requestId,
      mode: type,
      action: restarting ? "restart" : known.started ? "more" : "first",
    });
    try {
      const result = await withDeadline(
        request.signal,
        REQUEST_DEADLINE_MS,
        (signal) =>
          artistDiscographyApi.read(
            account,
            recording,
            association.id,
            {
              limit: PAGE_LIMIT,
              continuation: restarting ? null : known.continuation,
              release_type: type,
            },
            signal,
          ),
      );
      if (!request.active()) return;
      if (result.status === "type_filter_unsupported") {
        outcome = "unsupported";
        update(type, (value) => ({ ...value, started: false }), {
          unsupported: true,
        });
        return;
      }
      if (result.status !== "available")
        throw new Error(discographyUnavailable(result.status));
      if (
        result.association_id !== association.id ||
        result.page.artist_id !== profile.artist.id
      )
        throw new Error(
          "The discography did not belong to this saved artist review. Load the review again.",
        );
      if (
        result.page.items.length > PAGE_LIMIT ||
        result.page.items.some((group) => !belongs(type, group.kind))
      )
        throw new Error(
          "The provider returned releases that do not match this type. Try again.",
        );
      const items = result.page.items;
      const next = result.continuation ?? null;
      count = items.length;
      update(type, (value) => {
        const known = new Set(value.groups.map((group) => group.id));
        const added = items.filter((group) => {
          if (known.has(group.id)) return false;
          known.add(group.id);
          return true;
        });
        const pages = value.pages + 1;
        return {
          groups: [...value.groups, ...added],
          continuation: pages >= MAX_PAGES_PER_TYPE ? null : next,
          started: true,
          pages,
          error: "",
          restartRequired: false,
        };
      });
      outcome = "accepted";
    } catch (error) {
      if (request.active()) {
        const timedOut = error instanceof DeadlineError;
        outcome = timedOut ? "timed_out" : "failed";
        update(type, (value) => ({
          ...value,
          error: errorMessage(error),
          restartRequired:
            value.restartRequired ||
            (error instanceof HttpError && error.status === 409),
        }));
      }
    } finally {
      owner.pending = false;
      if (request.active())
        setSnapshot((value) =>
          value.owner === owner ? { ...value, pending: null } : value,
        );
      (outcome === "failed" || outcome === "timed_out" ? log.warn : log.debug)(
        "artist_discography_type.finished",
        {
          request_id: requestId,
          mode: type,
          reason: outcome,
          count,
          duration_ms: Math.round(performance.now() - started),
        },
      );
    }
  }

  return {
    types: current.types,
    unsupported: current.unsupported,
    pending: current.pending,
    groups: Object.values(current.types).flatMap((paging) => paging.groups),
    more,
  };
}
