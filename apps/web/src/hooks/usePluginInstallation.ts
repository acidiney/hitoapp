import { useEffect, useMemo, useRef, useState } from "react";
import type { PluginActions } from "./usePlugins";
import { useRequestScope } from "./useRequestScope";
import type { PluginInstallationPreview, PluginService } from "../types";
import { errorMessage } from "../format";
import { log } from "../log";
import { createUuid } from "../uuid";

export function usePluginInstallation(
  services: PluginService[],
  actions: PluginActions,
) {
  const [draft, setDraft] = useState({
    serviceId: services[0]?.id ?? "",
    revision: services[0]?.revision ?? 0,
    token: "",
  });
  const service =
    services.find((item) => item.id === draft.serviceId) ?? services[0];
  const serviceId = service?.id ?? "";
  const revision = service?.revision ?? 0;
  const token =
    draft.serviceId === serviceId && draft.revision === revision
      ? draft.token
      : "";
  const scope = useRequestScope(`${serviceId}:${revision}`);
  const owner = useMemo(() => ({ generation: 0 }), [scope]);
  const operation = useRef<AbortController | null>(null);
  const [review, setReview] = useState<{
    owner: typeof owner;
    generation: number;
    preview: PluginInstallationPreview;
    operationId: string;
  } | null>(null);
  const [pending, setPending] = useState<typeof owner | null>(null);
  const [failure, setFailure] = useState<{
    owner: typeof owner;
    message: string;
  } | null>(null);
  const visibleReview =
    review?.owner === owner && review.generation === owner.generation
      ? review
      : null;
  useEffect(() => {
    setDraft((current) =>
      current.serviceId === serviceId && current.revision === revision
        ? current
        : { serviceId, revision, token: "" },
    );
  }, [serviceId, revision]);

  function invalidate() {
    owner.generation++;
    operation.current?.abort();
    operation.current = null;
    setReview(null);
    setPending(null);
    setFailure(null);
  }

  async function run(accept: boolean) {
    const request = scope.capture();
    if (!service || !request.active() || actions.pending || pending === owner)
      return;
    if (
      accept &&
      (!review ||
        review.owner !== owner ||
        review.generation !== owner.generation)
    )
      return;
    const accepted = accept ? review : null;
    invalidate();
    const controller = new AbortController();
    operation.current = controller;
    const signal = AbortSignal.any([request.signal, controller.signal]);
    const generation = owner.generation;
    const active = () =>
      request.active() && !signal.aborted && owner.generation === generation;
    const operationId = createUuid();
    const started = performance.now();
    const fields = {
      operation_id: operationId,
      action: accept ? "install" : "preview",
    };
    log.debug("plugins.installation.started", fields);
    setPending(owner);
    try {
      const body = {
        service_id: service.id,
        service_revision: service.revision,
        bearer_token: token || null,
      };
      if (accepted) {
        const result = await actions.install(
          { ...body, accept_manifest_digest: accepted.preview.digest },
          signal,
        );
        if (!active()) return;
        if (result) setDraft({ serviceId, revision, token: "" });
        log.info("plugins.installation.completed", {
          ...fields,
          reason: result ? "connected" : "failed",
          duration_ms: Math.round(performance.now() - started),
        });
      } else {
        const preview = await actions.preview(body, signal);
        if (!active() || !preview) return;
        setReview({ owner, generation, preview, operationId });
        log.info("plugins.installation.completed", {
          ...fields,
          reason: "review_required",
          duration_ms: Math.round(performance.now() - started),
        });
      }
    } catch (error) {
      if (active()) {
        setFailure({ owner, message: errorMessage(error) });
        log.warn("plugins.installation.failed", {
          ...fields,
          reason: "unavailable",
          duration_ms: Math.round(performance.now() - started),
        });
      }
    } finally {
      if (!active())
        log.debug("plugins.installation.cancelled", {
          ...fields,
          reason: "cancelled",
          duration_ms: Math.round(performance.now() - started),
        });
      if (operation.current === controller) operation.current = null;
      if (active()) setPending(null);
    }
  }

  return {
    service,
    token,
    review: visibleReview?.preview ?? null,
    pending: pending === owner,
    error: failure?.owner === owner ? failure.message : "",
    select: (id: string) => {
      invalidate();
      setDraft({
        serviceId: id,
        revision: services.find((item) => item.id === id)?.revision ?? 0,
        token: "",
      });
    },
    setToken: (value: string) => {
      invalidate();
      setDraft({ serviceId, revision, token: value });
    },
    preview: () => run(false),
    accept: () => run(true),
    back: () => {
      if (visibleReview)
        log.info("plugins.installation.declined", {
          operation_id: visibleReview.operationId,
          action: "preview",
          reason: "declined",
        });
      invalidate();
    },
  };
}
