import { useEffect, useState } from "react";
import type { Schema } from "../../../shared/schema";
import { log } from "../../../shared/log";
import { artistEditionApi } from "../api/artistEditionApi";
import { rememberReleaseGroups } from "../api/catalogCache";

type Found = { scope: string; group: Schema<"CatalogReleaseGroup"> };

export function useCatalogReleaseGroup(
  account: string,
  recording: string,
  association: Schema<"ArtistAssociation"> | null,
  releaseGroupId: string,
) {
  const associationId = association?.id ?? null;
  const scope = JSON.stringify([
    account,
    recording,
    associationId,
    releaseGroupId,
  ]);
  const [found, setFound] = useState<Found | null>(null);

  useEffect(() => {
    if (associationId === null) return;
    const controller = new AbortController();
    const started = performance.now();
    let reason = "cancelled";
    artistEditionApi
      .group(
        account,
        recording,
        associationId,
        { release_group_id: releaseGroupId },
        controller.signal,
      )
      .then((result) => {
        if (controller.signal.aborted) return;
        if (result.status !== "available") {
          reason = result.status;
          return;
        }
        if (
          result.association_id !== associationId ||
          result.group.id !== releaseGroupId
        ) {
          reason = "mismatched";
          return;
        }
        reason = "accepted";
        rememberReleaseGroups(account, associationId, [result.group]);
        setFound({ scope, group: result.group });
      })
      .catch(() => {
        if (!controller.signal.aborted) reason = "failed";
      })
      .finally(() => {
        const record = reason === "failed" ? log.warn : log.debug;
        record("catalog_release_group.finished", {
          reason,
          duration_ms: Math.round(performance.now() - started),
        });
      });
    return () => controller.abort();
  }, [account, recording, associationId, releaseGroupId, scope]);

  return found?.scope === scope ? found.group : null;
}
