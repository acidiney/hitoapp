import { chromium, type FullConfig } from "@playwright/test";
import {
  adminSessionPath,
  enterAdmin,
  sharedAdminSession,
} from "./fixtures/admin-session";

export default async function globalSetup(config: FullConfig) {
  if (!sharedAdminSession) return;
  const baseURL = config.projects[0].use.baseURL;
  if (!baseURL) throw new Error("The browser tests need a base URL");
  const browser = await chromium.launch();
  try {
    const context = await browser.newContext({
      baseURL,
      storageState: { cookies: [], origins: [] },
    });
    await enterAdmin(await context.newPage());
    const copies = await context.request.put(
      "/api/v1/account/automatic-copies",
      {
        headers: { "x-hito-request": "1", origin: new URL(baseURL).origin },
        data: { keep_played_tracks: false },
      },
    );
    if (!copies.ok())
      throw new Error("The shared administrator must not keep played tracks");
    await context.storageState({ path: adminSessionPath });
  } finally {
    await browser.close();
  }
}
