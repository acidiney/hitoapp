import type { BrowserContext, Page } from "@playwright/test";

type Probe = {
  supported: boolean;
  handlerCount(): number;
  position(): MediaPositionState | null;
  invoke(
    action: MediaSessionAction,
    details?: Partial<MediaSessionActionDetails>,
  ): boolean;
  remember(): void;
  invokeHeld(): void;
  armLogout(): boolean;
};
export type LogoutEvidence = {
  armed: boolean;
  beforeCleared: boolean;
  heldPresent: boolean;
  heldInvoked: boolean;
  afterCleared: boolean;
};
declare global {
  interface Window {
    __hitoOwnedMediaSession?: Probe;
  }
}

export async function observeMediaSession(context: BrowserContext) {
  let failures = 0;
  let armedOnce = false;
  let waiting:
    | {
        page: Page;
        finish(evidence?: LogoutEvidence): void;
      }
    | undefined;
  await context.exposeBinding(
    "__hitoOwnedMediaSessionLogout",
    (source, value: unknown) => {
      const current = waiting;
      const fields = [
        "armed",
        "beforeCleared",
        "heldPresent",
        "heldInvoked",
        "afterCleared",
      ];
      const valid =
        value !== null &&
        typeof value === "object" &&
        !Array.isArray(value) &&
        Object.keys(value).sort().join() === fields.sort().join() &&
        Object.values(value).every((field) => typeof field === "boolean");
      if (
        !current ||
        source.page !== current.page ||
        source.frame !== current.page.mainFrame() ||
        !valid
      ) {
        failures++;
        current?.finish();
        return;
      }
      current.finish(value as LogoutEvidence);
    },
  );
  await context.addInitScript(() => {
    const session = navigator.mediaSession;
    const supported =
      !!session &&
      typeof MediaMetadata === "function" &&
      typeof session.setActionHandler === "function" &&
      typeof session.setPositionState === "function";
    const handlers = new Map<MediaSessionAction, MediaSessionActionHandler>();
    let held: [MediaSessionAction, MediaSessionActionHandler][] = [];
    let position: MediaPositionState | null = null;
    let logoutArmed = false;
    const cleared = () =>
      session.metadata === null &&
      session.playbackState === "none" &&
      handlers.size === 0 &&
      position === null;
    function reportLogout() {
      if (!logoutArmed || !cleared()) return;
      const evidence: LogoutEvidence = {
        armed: logoutArmed,
        beforeCleared: cleared(),
        heldPresent: held.length > 0,
        heldInvoked: false,
        afterCleared: false,
      };
      logoutArmed = false;
      try {
        for (const [action, handler] of held)
          handler({ action, seekTime: 30, seekOffset: 5 });
        evidence.heldInvoked = true;
      } catch {}
      evidence.afterCleared = cleared();
      const deliver = (globalThis as unknown as Record<string, unknown>)
        .__hitoOwnedMediaSessionLogout;
      if (typeof deliver === "function")
        void Promise.resolve(deliver(evidence)).catch(() => {});
    }
    if (supported) {
      const action = session.setActionHandler.bind(session);
      session.setActionHandler = (name, handler) => {
        action(name, handler);
        if (handler) handlers.set(name, handler);
        else handlers.delete(name);
      };
      const updatePosition = session.setPositionState.bind(session);
      session.setPositionState = (value) => {
        updatePosition(value);
        position = value && Object.keys(value).length ? { ...value } : null;
        if (position === null) reportLogout();
      };
    }
    Object.defineProperty(window, "__hitoOwnedMediaSession", {
      value: {
        supported,
        handlerCount: () => handlers.size,
        position: () => position,
        invoke: (
          action: MediaSessionAction,
          details: Partial<MediaSessionActionDetails> = {},
        ) => {
          const handler = handlers.get(action);
          if (!handler) return false;
          handler({ action, ...details });
          return true;
        },
        remember: () => {
          held = [...handlers];
        },
        invokeHeld: () => {
          for (const [action, handler] of held)
            handler({ action, seekTime: 30, seekOffset: 5 });
        },
        armLogout: () => {
          if (
            !supported ||
            logoutArmed ||
            !held.length ||
            session.metadata === null ||
            position === null
          )
            return false;
          logoutArmed = true;
          return true;
        },
      } satisfies Probe,
    });
  });
  return {
    failures: () => failures,
    async armLogout(page: Page) {
      if (waiting || armedOnce)
        throw new Error("Only one owned logout observation may be armed");
      armedOnce = true;
      let finish!: (evidence?: LogoutEvidence) => void;
      const result = new Promise<LogoutEvidence>((resolve, reject) => {
        const timer = setTimeout(() => finish(), 8000);
        finish = (evidence) => {
          clearTimeout(timer);
          waiting = undefined;
          if (evidence) resolve(evidence);
          else
            reject(
              new Error(
                "The owned logout cleanup observation was absent or invalid",
              ),
            );
        };
      });
      void result.catch(() => {});
      waiting = { page, finish };
      try {
        const armed = await page.evaluate(
          () => window.__hitoOwnedMediaSession?.armLogout() === true,
        );
        if (!armed)
          throw new Error(
            "The owned logout cleanup observer could not be armed",
          );
      } catch (error) {
        finish();
        throw error;
      }
      return { result };
    },
  };
}

export async function systemAction(
  page: Page,
  action: MediaSessionAction,
  details: Partial<MediaSessionActionDetails> = {},
) {
  await page.evaluate(
    ({ action, details }) => {
      const button = document.createElement("button");
      button.textContent = "Owned system action";
      button.style.cssText = "position:fixed;left:0;top:0;z-index:2147483647";
      button.addEventListener(
        "click",
        () => {
          window.__hitoOwnedMediaSession?.invoke(action, details);
          button.remove();
        },
        { once: true },
      );
      document.body.append(button);
    },
    { action, details },
  );
  await page
    .getByRole("button", { name: "Owned system action", exact: true })
    .click();
}
