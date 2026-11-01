import type { Schema } from "../../../shared/schema";
import { useEffect, useState } from "react";
import { log } from "../../../shared/log";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { createUuid } from "../../../shared/uuid";
import { featuredApi } from "../api/featuredApi";

export function useFeaturedShelf(account: string): Schema<"FeaturedEntry">[] {
  const scope = useRequestScope(account);
  const [items, setItems] = useState<Schema<"FeaturedEntry">[]>([]);

  useEffect(() => {
    const request = scope.capture();
    const requestId = createUuid();
    const started = performance.now();
    let outcome = "failed";
    log.debug("featured_shelf.started", { request_id: requestId });
    featuredApi
      .read(account, request.signal)
      .then((shelf) => {
        if (!request.active()) {
          outcome = "cancelled";
          return;
        }
        outcome = shelf.status;
        if (shelf.status === "available") setItems(shelf.items);
      })
      .catch(() => {
        if (request.signal.aborted) outcome = "cancelled";
      })
      .finally(() => {
        log.debug("featured_shelf.finished", {
          request_id: requestId,
          reason: outcome,
          duration_ms: Math.round(performance.now() - started),
        });
      });
  }, [account, scope]);

  return items;
}
