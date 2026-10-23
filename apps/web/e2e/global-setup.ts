import { request, type FullConfig } from "@playwright/test";
import {
  adminCredentials,
  adminSessionPath,
  sharedAdminSession,
} from "./fixtures/admin-session";

export default async function globalSetup(config: FullConfig) {
  if (!sharedAdminSession) return;
  const baseURL = config.projects[0].use.baseURL;
  if (!baseURL) throw new Error("The browser tests need a base URL");
  const api = await request.newContext({
    baseURL,
    extraHTTPHeaders: {
      "x-hito-request": "1",
      origin: new URL(baseURL).origin,
    },
  });
  try {
    const setup = await api.get("/api/v1/setup");
    if (!setup.ok()) throw new Error("Server setup status is unavailable");
    const { needs_setup } = (await setup.json()) as { needs_setup: boolean };
    const entered = await api.post(
      needs_setup ? "/api/v1/setup" : "/api/v1/login",
      { data: adminCredentials },
    );
    if (!entered.ok()) throw new Error("The administrator could not sign in");
    await api.storageState({ path: adminSessionPath });
  } finally {
    await api.dispose();
  }
}
