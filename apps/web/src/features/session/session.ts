import { createUuid } from "../../shared/uuid";
import { router } from "@inertiajs/react";
import { log } from "../../shared/log";

const revisionKey = "hito-session-revision";
let channel: BroadcastChannel | undefined;

function revision() {
  try {
    return localStorage.getItem(revisionKey);
  } catch {
    return null;
  }
}

function account(props: { [key: string]: unknown }) {
  const user = props.user;
  return user &&
    typeof user === "object" &&
    "id" in user &&
    typeof user.id === "string"
    ? user.id
    : null;
}

export function notifySessionChanged() {
  const value = createUuid();
  try {
    localStorage.setItem(revisionKey, value);
  } catch {}
  channel?.postMessage(value);
}

export function installSessionBoundary(
  props: { [key: string]: unknown },
  dispose: () => void,
) {
  const initialAccount = account(props);
  const initialRevision = revision();
  const verification = new AbortController();
  let invalidated = false;
  function invalidate(reason: string) {
    if (invalidated) return;
    invalidated = true;
    cleanup();
    log.info("session.invalidated", { reason });
    router.cancelAll();
    router.clearHistory();
    dispose();
    window.location.replace("/library");
  }
  function checkRevision() {
    if (revision() !== initialRevision) invalidate("session_changed");
  }
  function storage(event: StorageEvent) {
    if (event.key === revisionKey) checkRevision();
  }
  function restored(event: PageTransitionEvent) {
    if (event.persisted) invalidate("page_restored");
    else checkRevision();
  }
  const observedAccount = (props: { [key: string]: unknown }) => {
    if (Object.hasOwn(props, "user") && account(props) !== initialAccount)
      invalidate("account_changed");
  };
  const stopBeforeUpdate = router.on("beforeUpdate", (event) =>
    observedAccount(event.detail.page.props),
  );
  const stopNavigate = router.on("navigate", (event) =>
    observedAccount(event.detail.page.props),
  );
  if (typeof BroadcastChannel !== "undefined") {
    channel = new BroadcastChannel(revisionKey);
    channel.onmessage = () => invalidate("session_changed");
  }
  window.addEventListener("storage", storage);
  window.addEventListener("focus", checkRevision);
  window.addEventListener("pageshow", restored);
  document.addEventListener("visibilitychange", checkRevision);
  function cleanup() {
    verification.abort();
    stopBeforeUpdate();
    stopNavigate();
    channel?.close();
    channel = undefined;
    window.removeEventListener("storage", storage);
    window.removeEventListener("focus", checkRevision);
    window.removeEventListener("pageshow", restored);
    document.removeEventListener("visibilitychange", checkRevision);
  }
  async function verify() {
    try {
      const response = await fetch("/api/v1/me", {
        credentials: "same-origin",
        cache: "no-store",
        signal: verification.signal,
      });
      let currentAccount: string | null = null;
      if (response.ok) {
        const user: unknown = await response.json();
        currentAccount = account({ user });
        if (!currentAccount) throw new Error("Invalid session response");
      } else if (response.status !== 401) {
        throw new Error("Session check failed");
      }
      log.debug("session.verified", {
        status: response.status,
        request_id: response.headers.get("x-request-id") ?? undefined,
      });
      if (currentAccount !== initialAccount)
        invalidate("initial_account_changed");
      checkRevision();
      return !invalidated;
    } catch {
      if (invalidated) return false;
      log.warn("session.verification_failed", { reason: "connection" });
      throw new Error(
        "Your session could not be verified. Check your connection, then reload.",
      );
    }
  }
  return { ready: verify() };
}
