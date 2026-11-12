import type { Browser } from "@playwright/test";
import { expect } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";

export const memberPassword = "owned browser member password";

export async function createMembers(
  browser: Browser,
  baseURL: string,
  prefix: string,
  count: number,
) {
  const headers = { "x-hito-request": "1", origin: new URL(baseURL).origin };
  const context = await browser.newContext({ baseURL });
  const administrator = context.request;
  const login = await administrator.post("/api/v1/login", {
    headers,
    data: { username: "admin", password: "docker browser test password" },
  });
  expect(login.status()).toBe(200);
  const libraries: Schema<"Library">[] = await (
    await administrator.get("/api/v1/libraries")
  ).json();
  const shared = libraries.find(
    (library: { personal?: boolean }) => !library.personal,
  )!;
  const stamp = Date.now() % 1_000_000;
  const names: string[] = [];
  for (let index = 0; index < count; index++) {
    const username = `${prefix}-${index}-${stamp}`;
    const created = await administrator.post("/api/v1/users", {
      headers,
      data: { username, password: memberPassword },
    });
    expect(created.status()).toBe(201);
    const member: Schema<"User"> = await created.json();
    const joined = await administrator.put(
      `/api/v1/libraries/${shared.id}/members/${member.id}`,
      { headers },
    );
    expect(joined.ok()).toBe(true);
    names.push(username);
  }
  return { names, administrator, close: () => context.close() };
}
