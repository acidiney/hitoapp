import type { Browser } from "@playwright/test";
import {
  adminSessionPath,
  enterAdmin,
  sharedAdminSession,
} from "./admin-session";
import { connectFixtures } from "./streaming-quality";

export async function ensureFixtureLibrary(browser: Browser, baseURL: string) {
  const context = await browser.newContext({
    baseURL,
    storageState: sharedAdminSession ? adminSessionPath : undefined,
  });
  try {
    const page = await context.newPage();
    if (!sharedAdminSession) await enterAdmin(page);
    await connectFixtures(page, baseURL);
  } finally {
    await context.close();
  }
}
