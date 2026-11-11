import { log } from "../shared/log";

export type ServiceWorkerEnvironment = {
  production: boolean;
  secureContext: boolean;
  container: ServiceWorkerContainer | undefined;
};

export function currentEnvironment(): ServiceWorkerEnvironment {
  return {
    production: import.meta.env.PROD,
    secureContext: typeof window !== "undefined" && window.isSecureContext,
    container:
      typeof navigator !== "undefined" ? navigator.serviceWorker : undefined,
  };
}

export function canRegisterServiceWorker(
  environment: ServiceWorkerEnvironment,
) {
  return (
    environment.production &&
    environment.secureContext &&
    environment.container !== undefined
  );
}

export function reloadOnceControlled(
  container: ServiceWorkerContainer,
  reload: () => void,
) {
  let reloading = false;
  container.addEventListener("controllerchange", () => {
    if (reloading) return;
    reloading = true;
    reload();
  });
}

export function applyUpdate(
  worker: ServiceWorker,
  container: ServiceWorkerContainer,
  reload: () => void,
) {
  reloadOnceControlled(container, reload);
  worker.postMessage({ type: "SKIP_WAITING" });
}

export async function registerServiceWorker(
  environment: ServiceWorkerEnvironment,
  onUpdateReady: (apply: () => void) => void,
  reload: () => void = () => window.location.reload(),
): Promise<boolean> {
  const container = environment.container;
  if (!canRegisterServiceWorker(environment) || !container) return false;
  try {
    const registration = await container.register("/sw.js", { scope: "/" });
    const announce = (worker: ServiceWorker) =>
      onUpdateReady(() => applyUpdate(worker, container, reload));
    if (registration.waiting && container.controller) {
      announce(registration.waiting);
    }
    registration.addEventListener("updatefound", () => {
      const installing = registration.installing;
      installing?.addEventListener("statechange", () => {
        if (installing.state === "installed" && container.controller) {
          announce(installing);
        }
      });
    });
    log.info("service_worker.registered");
    return true;
  } catch {
    log.warn("service_worker.registration_failed");
    return false;
  }
}
