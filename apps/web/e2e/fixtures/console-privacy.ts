import type { BrowserContext } from "@playwright/test";

type Capture = "text" | "snapshot";
type Failure = "text" | "serialization" | "binding";

export async function captureConsolePrivacy(
  context: BrowserContext,
  inspect: (text: string, capture: Capture) => void,
  failed: (failure: Failure) => void,
  pending: Set<Promise<void>>,
) {
  const binding = "__hitoOwnedConsoleSnapshot";
  const bindingFailure = "Owned console snapshot delivery failed";
  context.on("console", (message) => {
    try {
      const text = message.text();
      inspect(text, "text");
      if (text === bindingFailure) failed("binding");
    } catch {
      failed("text");
    }
  });
  await context.exposeBinding(
    binding,
    (_source, serialized: unknown, snapshot: unknown) => {
      const checking = Promise.resolve()
        .then(() => {
          if (serialized !== true || typeof snapshot !== "string") {
            failed("serialization");
            return;
          }
          inspect(snapshot, "snapshot");
        })
        .catch(() => failed("binding"))
        .finally(() => pending.delete(checking));
      pending.add(checking);
      return checking;
    },
  );
  await context.addInitScript(
    ({ binding, bindingFailure }) => {
      const target = globalThis as unknown as Record<string, unknown>;
      const send = target[binding];
      const nativeError = console.error.bind(console);
      const deliveryFailed = () => nativeError(bindingFailure);
      if (typeof send !== "function") {
        deliveryFailed();
        return;
      }
      const methods = console as unknown as Record<string, unknown>;
      for (const name of Object.getOwnPropertyNames(console)) {
        const original = methods[name];
        if (typeof original !== "function") continue;
        methods[name] = (...args: unknown[]) => {
          let serialized = true;
          let snapshot = "";
          try {
            snapshot = JSON.stringify(args, (_key, value: unknown) => {
              if (
                value !== null &&
                typeof value === "object" &&
                Object.prototype.toString.call(value) === "[object Error]"
              ) {
                const error = value as Error;
                return {
                  ...error,
                  name: error.name,
                  message: error.message,
                  stack: error.stack,
                };
              }
              if (
                typeof value === "bigint" ||
                typeof value === "symbol" ||
                typeof value === "function"
              )
                throw new Error("Console argument cannot be inspected");
              return value;
            });
          } catch {
            serialized = false;
          }
          try {
            Promise.resolve(send(serialized, snapshot)).catch(deliveryFailed);
          } catch {
            deliveryFailed();
          }
          return Reflect.apply(original, console, args);
        };
      }
    },
    { binding, bindingFailure },
  );
}
