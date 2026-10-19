import { randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { request, type APIRequestContext } from "@playwright/test";
import type { components } from "../../src/generated";

type Schema = components["schemas"];
export type LibraryBudget = { requests: number; writes: number };
export type Remember = (value: string) => void;

export async function libraryClient(
  origin: string,
  credentials: Schema["Credentials"],
  budget: LibraryBudget,
  remember: Remember,
) {
  const api: APIRequestContext = await request.newContext({
    baseURL: origin,
    timeout: 10_000,
  });
  let account: string | null = null;
  async function call<T>(
    method: string,
    path: string,
    status = 200,
    data?: unknown,
  ): Promise<T> {
    if (++budget.requests > 180)
      throw new Error("The library public fixture request budget was exceeded");
    if (!["GET", "HEAD"].includes(method)) budget.writes++;
    if (data !== undefined && Buffer.byteLength(JSON.stringify(data)) > 65_536)
      throw new Error(
        "The library public fixture request exceeded its body bound",
      );
    const response = await api.fetch(path, {
      method,
      data,
      headers: {
        Origin: origin,
        "x-hito-request": "1",
        ...(account ? { "x-hito-account": account } : {}),
      },
    });
    try {
      const bytes = await response.body();
      if (response.status() !== status || bytes.length > 524_288)
        throw new Error("The library public fixture response was invalid");
      return bytes.length ? (JSON.parse(bytes.toString()) as T) : (null as T);
    } finally {
      await response.dispose();
    }
  }
  try {
    const session = await call<Schema["AuthSession"]>(
      "POST",
      "/api/v1/login",
      200,
      credentials,
    );
    account = session.user.id;
    remember(session.token);
    return {
      user: session.user,
      read: <T>(path: string) => call<T>("GET", path),
      call,
      async sync(actions: Schema["Action"][]) {
        if (actions.length > 400)
          throw new Error("The library fixture operation budget was exceeded");
        const accepted: Schema["Operation"][] = [];
        for (let offset = 0; offset < actions.length; offset += 100) {
          const batch: Schema["SyncBatch"] = {
            device_id: "owned-library-browser-fixture",
            operations: actions
              .slice(offset, offset + 100)
              .map((action) => ({ id: randomUUID(), action })),
          };
          const receipt = await call<Schema["SyncReceipt"]>(
            "POST",
            "/api/v1/sync",
            200,
            batch,
          );
          if (
            !isDeepStrictEqual(
              receipt.acknowledged,
              batch.operations.map((operation) => operation.id),
            )
          )
            throw new Error(
              "The library fixture operation acknowledgement changed",
            );
          accepted.push(...batch.operations);
        }
        return accepted;
      },
      close: () => api.dispose(),
    };
  } catch (error) {
    await api.dispose();
    throw error;
  }
}
export type LibraryClient = Awaited<ReturnType<typeof libraryClient>>;
