import { randomUUID } from "node:crypto";
import { mkdirSync } from "node:fs";
import { expect, test } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import {
  collaborationCredits,
  collaborationProvider,
} from "./fixtures/collaboration-credits-provider";

const SHOTS = "test-results/collab-credits";

test("every member of a collaboration lists it and a featured artist lists it as an appearance", async ({
  page,
  baseURL,
}) => {
  const label = randomUUID().slice(0, 8);
  const credits = collaborationCredits(label);
  const fixture = await collaborationProvider(label, credits);
  const headers = { "x-hito-request": "1", origin: new URL(baseURL!).origin };
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const send = async <T>(path: string, data: unknown, status = 201) => {
    const response = await page.request.fetch(path, {
      method: "POST",
      headers,
      data,
    });
    expect(response.status(), path).toBe(status);
    return (await response.json()) as T;
  };
  let connection: Schema<"PluginInstallation"> | null = null;
  try {
    const service = await send<Schema<"PluginService">>(
      "/api/v1/plugins/services",
      {
        name: `Collaboration ${label}`,
        base_url: fixture.address,
      } satisfies Schema<"OfferPluginService">,
    );
    connection = await send<Schema<"PluginInstallation">>("/api/v1/plugins", {
      service_id: service.id,
      service_revision: service.revision,
      bearer_token: fixture.token,
    } satisfies Schema<"InstallPlugin">);
    for (const id of Object.keys(credits.recordings))
      await send(
        `/api/v1/plugins/${connection.id}/recordings`,
        {
          installation_revision: connection.revision,
          provider_recording_id: id,
        } satisfies Schema<"AdoptProviderRecording">,
        200,
      );
    const artists = (await (
      await page.request.get(`/api/v1/artists?q=${label}`)
    ).json()) as Schema<"Page_Artist">;
    const named = (name: string) =>
      artists.items.find((artist) => artist.name === name)!;
    expect(named(`${credits.ada} & ${credits.bo}`).recording_count).toBe(2);
    expect(named(credits.ada).recording_count).toBe(3);
    expect(named(credits.bo).recording_count).toBe(2);
    expect(named(credits.cy).recording_count).toBe(0);

    const combined = `${credits.ada} & ${credits.bo}`;
    const track = (title: string) =>
      page.getByRole("link", { name: title, exact: true });
    mkdirSync(SHOTS, { recursive: true });
    await page.emulateMedia({ colorScheme: "dark" });
    for (const [size, viewport] of [
      ["desktop", { width: 1440, height: 1000 }],
      ["phone", { width: 390, height: 1400 }],
    ] as const) {
      await page.setViewportSize(viewport);
      await page.goto(`/artists/${named(credits.bo).id}`);
      await expect(track(`Together ${label}`)).toBeVisible();
      await expect(track(`Reordered ${label}`)).toBeVisible();
      await expect(track(`Guest verse ${label}`)).toHaveCount(0);
      const together = page.getByRole("row", {
        name: new RegExp(`Together ${label}`),
      });
      await expect(
        together.getByRole("link", { name: credits.ada }).first(),
      ).toHaveAttribute("href", `/artists/${named(credits.ada).id}`);
      await expect(
        together.getByRole("link", { name: credits.bo }).first(),
      ).toHaveAttribute("href", `/artists/${named(credits.bo).id}`);
      await expect(together.getByRole("link", { name: combined })).toHaveCount(
        0,
      );
      await expect(together.locator("td.artist-column")).toHaveText(combined);
      await page.screenshot({
        path: `${SHOTS}/after-bo-${size}.png`,
      });
      await page.goto(`/library?q=${label}`);
      await expect(track(`Together ${label}`)).toBeVisible();
      await page.screenshot({ path: `${SHOTS}/after-library-${size}.png` });
      await page.goto(`/artists/${named(credits.cy).id}`);
      await expect(
        page.getByRole("heading", { name: "Appears on" }),
      ).toBeVisible();
      await expect(track(`Guest verse ${label}`)).toBeVisible();
      await expect(page.getByRole("heading", { name: "Tracks" })).toHaveCount(
        0,
      );
      const guest = page.getByRole("row", {
        name: new RegExp(`Guest verse ${label}`),
      });
      await expect(guest.locator("td.artist-column")).toHaveText(
        `${credits.ada} feat. ${credits.cy}`,
      );
      await expect(
        guest.getByRole("link", { name: credits.ada }).first(),
      ).toHaveAttribute("href", `/artists/${named(credits.ada).id}`);
      await expect(
        guest.getByRole("link", { name: credits.cy }).first(),
      ).toHaveAttribute("href", `/artists/${named(credits.cy).id}`);
      await page.screenshot({ path: `${SHOTS}/after-cy-${size}.png` });
    }
    await page.goto(`/artists/${named(credits.cy).id}`);
    await expect(page.getByRole("row")).toHaveCount(61);
    await expect(page.getByText("Showing 60 of 62 appearances")).toBeVisible();
    await page.getByRole("button", { name: "Load more" }).click();
    await expect(page.getByRole("row")).toHaveCount(63);
    await expect(track(`Zed 60 ${label}`)).toBeVisible();
    await expect(page.getByRole("button", { name: "Load more" })).toHaveCount(
      0,
    );
    await page.setViewportSize({ width: 1440, height: 1000 });
    await page.goto(`/artists/${named(credits.ada).id}`);
    const guestRow = page.getByRole("row", {
      name: new RegExp(`Guest verse ${label}`),
    });
    await expect(guestRow.locator("td.artist-column")).toHaveText("");
    await expect(guestRow.getByRole("link", { name: credits.ada })).toHaveCount(
      0,
    );
    await page
      .getByRole("row", { name: new RegExp(`Together ${label}`) })
      .locator("td.artist-column")
      .getByRole("link", { name: credits.bo })
      .click();
    await expect(page).toHaveURL(`/artists/${named(credits.bo).id}`);
    await expect(
      page.getByRole("heading", { name: credits.bo, level: 1 }),
    ).toBeVisible();
    await page.goto(`/artists/${named(credits.ada).id}`);
    for (const title of ["Together", "Reordered", "Guest verse"])
      await expect(track(`${title} ${label}`)).toBeVisible();
    await expect(page.getByRole("heading", { name: "Appears on" })).toHaveCount(
      0,
    );
    expect(fixture.state.unexpected).toBe(0);
    expect(errors).toEqual([]);
  } finally {
    if (connection)
      await page.request.delete(`/api/v1/plugins/${connection.id}`, {
        headers,
        data: { revision: connection.revision },
      });
    await fixture.close();
  }
});
