import {
  connectionRestoredEvent,
  reportUnreachable,
} from "../shared/connection";

type RecoveryEvent = { preventDefault: () => void; detail: unknown };

export type RecoveryRouter = {
  on: (
    name: "networkError" | "httpException",
    handler: (event: RecoveryEvent) => void,
  ) => () => void;
  reload: () => void;
};

const gatewayStatuses = [502, 503, 504];

function responseStatus(detail: unknown) {
  if (detail === null || typeof detail !== "object" || !("response" in detail))
    return undefined;
  const response = detail.response;
  return response !== null &&
    typeof response === "object" &&
    "status" in response &&
    typeof response.status === "number"
    ? response.status
    : undefined;
}

export function installConnectionRecovery(router: RecoveryRouter) {
  let visitFailed = false;
  const stopNetwork = router.on("networkError", (event) => {
    event.preventDefault();
    visitFailed = true;
    reportUnreachable("network");
  });
  const stopHttp = router.on("httpException", (event) => {
    const status = responseStatus(event.detail);
    if (status === undefined || !gatewayStatuses.includes(status)) return;
    event.preventDefault();
    visitFailed = true;
    reportUnreachable("gateway");
  });
  const restored = () => {
    if (!visitFailed) return;
    visitFailed = false;
    router.reload();
  };
  window.addEventListener(connectionRestoredEvent, restored);
  return () => {
    stopNetwork();
    stopHttp();
    window.removeEventListener(connectionRestoredEvent, restored);
  };
}
