import { useEffect, useMemo, useState } from "react";
import { artistDiscographyApi } from "../api/artistDiscographyApi";
import { log } from "../../../shared/log";
import type {
  ArtistAssociation,
  CatalogReleaseGroup,
  LibraryAlbum,
} from "../../../shared/types";
import { createUuid } from "../../../shared/uuid";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

const MAX_GROUPS = 50;
const REQUEST_DEADLINE_MS = 20_000;
const FAILED =
  "Library availability could not be checked. The catalog is still shown.";

type Owner = { pending: boolean };
type State = {
  owner: Owner;
  albums: Record<string, LibraryAlbum[]>;
  pending: boolean;
  failed: boolean;
};

function empty(owner: Owner): State {
  return { owner, albums: {}, pending: false, failed: false };
}

export function useDiscographyLibrary(
  account: string,
  recording: string,
  association: ArtistAssociation,
  groups: CatalogReleaseGroup[],
  enabled: boolean,
) {
  const scope = useRequestScope(
    JSON.stringify([account, recording, association.id]),
  );
  const owner = useMemo<Owner>(() => ({ pending: false }), [scope]);
  const [snapshot, setSnapshot] = useState(() => empty(owner));
  const current = snapshot.owner === owner ? snapshot : empty(owner);
  const missing = enabled
    ? groups.map((group) => group.id).filter((id) => !(id in current.albums))
    : [];

  async function load(ids: string[]) {
    const request = scope.capture();
    if (!request.active() || owner.pending || ids.length === 0) return;
    owner.pending = true;
    const requestId = createUuid();
    const started = performance.now();
    const controller = new AbortController();
    let stop!: (error: Error) => void;
    const stopped = new Promise<never>((_, reject) => {
      stop = reject;
    });
    const cancel = () => {
      const error = new DOMException(
        "The library check was cancelled",
        "AbortError",
      );
      controller.abort(error);
      stop(error);
    };
    request.signal.addEventListener("abort", cancel, { once: true });
    const timer = setTimeout(() => {
      const error = new Error("The library check timed out");
      controller.abort(error);
      stop(error);
    }, REQUEST_DEADLINE_MS);
    if (request.signal.aborted) cancel();
    setSnapshot((value) => ({
      ...(value.owner === owner ? value : empty(owner)),
      pending: true,
      failed: false,
    }));
    let outcome = "cancelled";
    try {
      const result = await Promise.race([
        artistDiscographyApi.library(
          account,
          recording,
          association.id,
          { release_group_ids: ids },
          controller.signal,
        ),
        stopped,
      ]);
      if (!request.active()) return;
      const returned = result.groups.map((group) => group.release_group_id);
      if (
        result.association_id !== association.id ||
        returned.length !== ids.length ||
        returned.some((id, index) => id !== ids[index])
      )
        throw new Error(
          "The library check did not match the releases asked for",
        );
      setSnapshot((value) =>
        request.active() && value.owner === owner
          ? {
              ...value,
              albums: {
                ...value.albums,
                ...Object.fromEntries(
                  result.groups.map((group) => [
                    group.release_group_id,
                    group.albums,
                  ]),
                ),
              },
              pending: false,
              failed: false,
            }
          : value,
      );
      outcome = "accepted";
    } catch {
      outcome = "failed";
      if (request.active())
        setSnapshot((value) =>
          request.active() && value.owner === owner
            ? { ...value, pending: false, failed: true }
            : value,
        );
    } finally {
      clearTimeout(timer);
      request.signal.removeEventListener("abort", cancel);
      owner.pending = false;
      (outcome === "failed" ? log.warn : log.debug)(
        "artist_discography_library.finished",
        {
          request_id: requestId,
          reason: outcome,
          count: ids.length,
          duration_ms: Math.round(performance.now() - started),
        },
      );
    }
  }

  const wanted = missing.slice(0, MAX_GROUPS);
  const key = wanted.join("\u0000");
  useEffect(() => {
    if (current.failed || current.pending || wanted.length === 0) return;
    let active = true;
    queueMicrotask(() => {
      if (active) void load(wanted);
    });
    return () => {
      active = false;
    };
  }, [owner, scope, key, current.failed, current.pending]);

  return {
    albums: current.albums,
    pending:
      current.pending || (enabled && missing.length > 0 && !current.failed),
    error: current.failed ? FAILED : "",
    retry: () => {
      setSnapshot((value) =>
        value.owner === owner ? { ...value, failed: false } : value,
      );
    },
  };
}
