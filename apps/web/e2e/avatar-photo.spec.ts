import { expect, test, type Page } from "@playwright/test";
import { deflateSync, crc32 } from "node:zlib";
import type { Schema } from "../src/shared/schema";

const memberPassword = "avatar photo member password";

function headers(baseURL: string) {
  return { "x-hito-request": "1", origin: new URL(baseURL).origin };
}

function chunk(type: string, data: Buffer) {
  const body = Buffer.concat([Buffer.from(type), data]);
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const checksum = Buffer.alloc(4);
  checksum.writeUInt32BE(crc32(body));
  return Buffer.concat([length, body, checksum]);
}

function syntheticPng(width: number, height: number) {
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header.set([8, 2, 0, 0, 0], 8);
  const rows = Buffer.alloc((width * 3 + 1) * height);
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const at = y * (width * 3 + 1) + 1 + x * 3;
      rows[at] = Math.round((x / width) * 255);
      rows[at + 1] = Math.round((y / height) * 255);
      rows[at + 2] = 140;
    }
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", header),
    chunk("IDAT", deflateSync(rows)),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

async function signIn(page: Page, username: string, password: string) {
  await page.goto("/");
  await page.getByLabel("Username", { exact: true }).fill(username);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
}

async function loaded(page: Page, selector: string) {
  const image = page.locator(selector).first();
  await expect(image).toBeVisible();
  await expect
    .poll(() => image.evaluate((node: HTMLImageElement) => node.naturalWidth))
    .toBe(256);
}

test("a member uploads, replaces and removes a photo that the menu, profile and Accounts list show", async ({
  page,
  browser,
  baseURL,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const libraries: Schema<"Library">[] = await (
    await page.request.get("/api/v1/libraries")
  ).json();
  const created = await page.request.post("/api/v1/users", {
    headers: headers(baseURL!),
    data: { username: "avatar-member", password: memberPassword },
  });
  expect(created.status()).toBe(201);
  const member: Schema<"User"> = await created.json();
  expect(
    (
      await page.request.put(
        `/api/v1/libraries/${libraries[0].id}/members/${member.id}`,
        { headers: headers(baseURL!) },
      )
    ).ok(),
  ).toBe(true);

  const context = await browser.newContext({
    baseURL,
    storageState: { cookies: [], origins: [] },
  });
  const memberPage = await context.newPage();
  memberPage.on("pageerror", (error) => errors.push(error.message));
  await signIn(memberPage, "avatar-member", memberPassword);
  const menu = memberPage.locator(".account-link .avatar");
  await expect(menu).toHaveText("AM");
  await memberPage.goto("/settings?section=profile");
  await expect(
    memberPage.getByRole("button", { name: "Remove photo" }),
  ).toHaveCount(0);

  const file = memberPage.getByLabel("Profile photo file");
  await file.setInputFiles({
    name: "synthetic.png",
    mimeType: "image/png",
    buffer: syntheticPng(640, 360),
  });
  await expect(memberPage.getByText("Photo saved.")).toBeVisible();
  await loaded(memberPage, ".account-link .avatar img");
  await loaded(memberPage, "#setting-account-photo .avatar img");
  await expect(menu).toHaveText("");
  await expect(
    memberPage.getByRole("button", { name: "Remove photo" }),
  ).toBeVisible();
  const source = await memberPage
    .locator(".account-link .avatar img")
    .getAttribute("src");
  expect(source).toContain(`/api/v1/accounts/${member.id}/avatar?v=`);

  await memberPage.reload();
  await loaded(memberPage, ".account-link .avatar img");

  await file.setInputFiles({
    name: "huge.png",
    mimeType: "image/png",
    buffer: Buffer.alloc(2 * 1024 * 1024 + 1),
  });
  await expect(memberPage.getByRole("alert")).toContainText("2 MiB");
  await file.setInputFiles({
    name: "notes.png",
    mimeType: "image/png",
    buffer: Buffer.from("not a picture"),
  });
  await expect(memberPage.getByRole("alert")).toContainText(
    "JPEG, PNG or WebP",
  );
  await expect(memberPage.locator(".account-link .avatar img")).toHaveCount(1);

  await file.setInputFiles({
    name: "second.png",
    mimeType: "image/png",
    buffer: syntheticPng(300, 500),
  });
  await expect(memberPage.getByText("Photo saved.")).toBeVisible();
  await expect
    .poll(() =>
      memberPage.locator(".account-link .avatar img").getAttribute("src"),
    )
    .not.toBe(source);

  await page.goto("/settings?section=accounts");
  const row = page.locator(".setting-line", { hasText: "avatar-member" });
  await loaded(page, ".setting-line:has-text('avatar-member') .avatar img");
  const mine = await page.request.get(`/api/v1/accounts/${member.id}/avatar`);
  expect(mine.status()).toBe(200);
  expect(mine.headers()["content-type"]).toBe("image/jpeg");
  expect(mine.headers()["cache-control"]).toBe("private, no-cache");

  await memberPage.getByRole("button", { name: "Remove photo" }).click();
  await expect(memberPage.getByText(/Photo removed/)).toBeVisible();
  await expect(menu).toHaveText("AM");
  await expect(memberPage.locator(".account-link .avatar img")).toHaveCount(0);
  await expect(
    memberPage.getByRole("button", { name: "Remove photo" }),
  ).toHaveCount(0);
  await page.reload();
  await expect(row.locator(".avatar")).toHaveText("AM");
  expect(
    (await page.request.get(`/api/v1/accounts/${member.id}/avatar`)).status(),
  ).toBe(404);

  await context.close();
  expect(errors).toEqual([]);
});
