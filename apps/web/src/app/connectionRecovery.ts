import {
  connectionRestoredEvent,
  markChangeLost,
  reportUnreachable,
} from "../shared/connection";

type RecoveryEvent = { preventDefault: () => void; detail: unknown };

export type RecoveryRouter = {
  on: (
    name: "before" | "networkError" | "httpException",
    handler: (event: RecoveryEvent) => void,
  ) => () => void;
  reload: () => void;
};

const gatewayStatuses = [502, 503, 504];
const reloadSpacingMs = 15_000;

function field(value: unknown, name: string): unknown {
  return value !== null && typeof value === "object" && name in value
    ? (value as Record<string, unknown>)[name]
    : undefined;
}

function carriesHitoError(data: unknown): boolean {
  if (typeof data === "string") {
    try {
      return carriesHitoError(JSON.parse(data));
    } catch {
      return false;
    }
  }
  return typeof field(data, "error") === "string";
}

export function installConnectionRecovery(router: RecoveryRouter) {
  let visitFailed = false;
  let method = "get";
  let lastReload = Number.NEGATIVE_INFINITY;
  const failed = (reason: "network" | "gateway") => {
    visitFailed = true;
    if (method !== "get") markChangeLost();
    reportUnreachable(reason);
  };
  const stopBefore = router.on("before", (event) => {
    const visit = field(event.detail, "visit");
    const requested = field(visit, "method");
    method = typeof requested === "string" ? requested.toLowerCase() : "get";
  });
  const stopNetwork = router.on("networkError", (event) => {
    event.preventDefault();
    failed("network");
  });
  const stopHttp = router.on("httpException", (event) => {
    const response = field(event.detail, "response");
    const status = field(response, "status");
    if (
      typeof status !== "number" ||
      !gatewayStatuses.includes(status) ||
      carriesHitoError(field(response, "data"))
    )
      return;
    event.preventDefault();
    failed("gateway");
  });
  const restored = () => {
    if (!visitFailed) return;
    visitFailed = false;
    const now = Date.now();
    if (now - lastReload < reloadSpacingMs) return;
    lastReload = now;
    router.reload();
  };
  window.addEventListener(connectionRestoredEvent, restored);
  return () => {
    stopBefore();
    stopNetwork();
    stopHttp();
    window.removeEventListener(connectionRestoredEvent, restored);
  };
}
