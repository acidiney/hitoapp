import { expect, test, type Page, type Route } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { openTrackMenu } from "./fixtures/track-row";
import { networkQuiet } from "./fixtures/settled";

const first = "Fixture 440 Hz";
const second = "Fixture 660 Hz";

function dock(page: Page) {
  return page.getByRole("contentinfo", { name: "Audio player", exact: true });
}

async function startQueue(page: Page) {
  await page.goto("/library?q=Hz&sort=title");
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "List view", exact: true }).click();
  const table = page.getByRole("table", { name: "Tracks", exact: true });
  const firstRow = table.locator("tbody tr").filter({ hasText: first });
  const secondRow = table.locator("tbody tr").filter({ hasText: second });
  await secondRow
    .getByRole("button", { name: `Play ${second}`, exact: true })
    .click();
  await expect
    .poll(() =>
      page
        .locator("audio[data-hito-player]")
        .evaluate((audio: HTMLAudioElement) => audio.currentTime),
    )
    .toBeGreaterThan(0.2);
  await (await openTrackMenu(firstRow, first))
    .getByRole("menuitem", { name: "Add to queue" })
    .click();
}

test("the player bar likes and manages the playing track, and a reload keeps the queue paused at the same track", async ({
  page,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));

  await startQueue(page);
  const bar = dock(page);
  const heart = bar.getByRole("button", {
    name: `Like ${second}`,
    exact: true,
  });
  await heart.click();
  const liked = bar.getByRole("button", {
    name: `Unlike ${second}`,
    exact: true,
  });
  await expect(liked).toHaveAttribute("aria-pressed", "true");

  const menu = await openTrackMenu(bar, second);
  await expect(menu.getByRole("menuitem")).toHaveText([
    "Track details",
    "Add to playlist",
    "Fix source",
  ]);
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);

  await bar.getByRole("button", { name: "Pause", exact: true }).click();
  await expect(
    bar.getByRole("button", { name: "Play", exact: true }),
  ).toBeVisible();
  const seek = bar.getByRole("slider", { name: "Seek" });
  const before = Number(await seek.inputValue());
  expect(before).toBeGreaterThan(0);
  const queueButton = bar.getByRole("button", { name: /^Open queue/ });
  const queued = (await queueButton.innerText()).trim();
  expect(queued).toMatch(/Queue · \d+/);

  await page.reload();
  await expect(
    bar.getByRole("link", { name: second, exact: true }),
  ).toBeVisible();
  await expect(
    bar.getByRole("button", { name: "Play", exact: true }),
  ).toBeVisible();
  await expect(queueButton).toHaveText(queued);
  expect(Math.abs(Number(await seek.inputValue()) - before)).toBeLessThan(0.6);
  expect(
    await page
      .locator("audio[data-hito-player]")
      .evaluate((audio: HTMLAudioElement) => audio.paused),
  ).toBe(true);
  await expect(liked).toHaveAttribute("aria-pressed", "true");

  await bar.getByRole("button", { name: /^Open queue/ }).click();
  await expect(
    page.getByRole("heading", { name: "Queue", exact: true, level: 2 }),
  ).toBeVisible();
  await expect(page.locator(".queue-now-row strong")).toHaveText(second);
  await expect(
    page.locator(".queue-next .queue-track strong").filter({ hasText: first }),
  ).toHaveCount(1);
  await expect(
    page
      .locator(".queue-next li")
      .getByRole("button", { name: `Actions for ${first}` }),
  ).toBeVisible();

  await bar.getByRole("button", { name: "Play", exact: true }).click();
  await expect
    .poll(() =>
      page
        .locator("audio[data-hito-player]")
        .evaluate((audio: HTMLAudioElement) => audio.currentTime),
    )
    .toBeGreaterThan(0.2);

  await liked.click();
  await expect(
    bar.getByRole("button", { name: `Like ${second}`, exact: true }),
  ).toHaveAttribute("aria-pressed", "false");
  await networkQuiet(page);
  expect(errors).toEqual([]);
});

async function measureNotice(page: Page) {
  return page.evaluate(() => {
    const box = (rect: DOMRect) => ({
      left: rect.left,
      top: rect.top,
      right: rect.right,
      bottom: rect.bottom,
    });
    const inside = (
      inner: { left: number; top: number; right: number; bottom: number },
      outer: { left: number; top: number; right: number; bottom: number },
    ) =>
      inner.left >= outer.left - 0.5 &&
      inner.top >= outer.top - 0.5 &&
      inner.right <= outer.right + 0.5 &&
      inner.bottom <= outer.bottom + 0.5;
    const overlaps = (
      a: { left: number; top: number; right: number; bottom: number },
      b: { left: number; top: number; right: number; bottom: number },
    ) =>
      a.left < b.right &&
      a.right > b.left &&
      a.top < b.bottom &&
      a.bottom > b.top;
    const footer = document.querySelector<HTMLElement>("footer.player");
    const notice = footer?.querySelector<HTMLElement>(".playback-status");
    const span = notice?.querySelector<HTMLElement>("span");
    const nav = document.querySelector<HTMLElement>(
      'nav[aria-label="Main navigation"]',
    );
    if (!footer || !notice || !span || !nav) {
      return { present: false } as const;
    }
    const range = document.createRange();
    range.selectNodeContents(span);
    const lines = Array.from(range.getClientRects()).map(box);
    const viewport = {
      left: 0,
      top: 0,
      right: window.innerWidth,
      bottom: window.innerHeight,
    };
    const clippers: {
      left: number;
      top: number;
      right: number;
      bottom: number;
    }[] = [];
    for (
      let element: HTMLElement | null = span.parentElement;
      element;
      element = element.parentElement
    ) {
      const style = getComputedStyle(element);
      if (style.overflowX !== "visible" || style.overflowY !== "visible") {
        const rect = element.getBoundingClientRect();
        const left = rect.left + element.clientLeft;
        const top = rect.top + element.clientTop;
        clippers.push({
          left,
          top,
          right: left + element.clientWidth,
          bottom: top + element.clientHeight,
        });
      }
    }
    let opacityProduct = 1;
    let chainVisible = true;
    for (
      let element: HTMLElement | null = span;
      element;
      element = element.parentElement
    ) {
      const style = getComputedStyle(element);
      opacityProduct *= Number(style.opacity);
      if (style.visibility !== "visible" || style.display === "none") {
        chainVisible = false;
      }
    }
    const lineChecks = lines.map((line) => {
      const y = (line.top + line.bottom) / 2;
      const points = [
        (line.left + line.right) / 2,
        line.left + 4,
        line.right - 4,
      ];
      return {
        line,
        positive: line.right - line.left > 0 && line.bottom - line.top > 0,
        inViewport: inside(line, viewport),
        inClippers: clippers.every((clipper) => inside(line, clipper)),
        hits: points.map((x) => {
          const top = document.elementsFromPoint(x, y)[0];
          return !!top && notice.contains(top);
        }),
      };
    });
    const style = getComputedStyle(notice);
    const noticeBox = box(notice.getBoundingClientRect());
    const rows = Array.from(
      footer.querySelectorAll<HTMLElement>(".now-playing, .transport-buttons"),
    ).map((row) => box(row.getBoundingClientRect()));
    const footerBox = box(footer.getBoundingClientRect());
    const navBox = box(nav.getBoundingClientRect());
    return {
      present: true,
      lineCount: lines.length,
      lineChecks,
      opacityProduct,
      chainVisible,
      clippers,
      fontSize: parseFloat(style.fontSize),
      noticeBox,
      rows,
      footerBox,
      navBox,
      overlapsRows: rows.some((row) => overlaps(noticeBox, row)),
      footerAboveNav: footerBox.bottom <= navBox.top + 0.5,
    } as const;
  });
}

test("phone playback status remains readable above controls and navigation", async ({
  page,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  await page.emulateMedia({ colorScheme: "dark" });
  await page.goto("/settings?section=music");
  const musicFolder = page
    .getByRole("list", { name: "Connected music folders" })
    .getByRole("listitem")
    .filter({ has: page.getByRole("heading", { name: "Music", exact: true }) });
  if ((await musicFolder.count()) === 0) {
    await page
      .getByRole("button", { name: "Add music folder", exact: true })
      .click();
    const picker = page.getByRole("dialog", {
      name: "Add a music folder",
      exact: true,
    });
    await picker
      .getByRole("button", { name: "Open Music", exact: true })
      .click();
    await picker
      .getByRole("button", { name: "Use this folder", exact: true })
      .click();
    await expect(picker).toBeHidden();
  }
  await expect(
    musicFolder.filter({ hasText: "1 file read · read-only" }),
  ).toHaveCount(1, { timeout: 20_000 });
  await page.goto("/library?q=Hz&sort=title");
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "No matching tracks", exact: true }),
  ).toHaveCount(0);
  await page.getByRole("button", { name: "List view", exact: true }).click();
  const row = page
    .getByRole("table", { name: "Tracks", exact: true })
    .locator("tbody tr")
    .filter({ hasText: first });
  await page.setViewportSize({ width: 390, height: 844 });
  const play = row.getByRole("button", { name: `Play ${first}`, exact: true });
  await expect(play).toBeVisible();
  await expect(play).toBeEnabled();

  let held = null as Route | null;
  let entered!: () => void;
  const reachedSources = new Promise<void>((resolve) => {
    entered = resolve;
  });
  const hold = async (route: Route) => {
    held = route;
    entered();
  };
  await page.route("**/api/v1/recordings/*/playback-sources**", hold);
  try {
    await row
      .getByRole("button", { name: `Play ${first}`, exact: true })
      .click();
    await reachedSources;
    const status = dock(page).locator(".playback-status span");
    await expect(status).toHaveText("Finding audio…");
    const measured = await measureNotice(page);
    await page.screenshot({ path: "test-results/phone-playback-status.png" });
    const message = JSON.stringify(measured);
    if (!measured.present)
      throw new Error(`notice or navigation missing ${message}`);
    expect(measured.lineCount, message).toBeGreaterThan(0);
    for (const check of measured.lineChecks) {
      expect(check.positive, message).toBe(true);
      expect(check.inViewport, message).toBe(true);
      expect(check.inClippers, message).toBe(true);
      expect(check.hits, message).toEqual([true, true, true]);
    }
    expect(measured.opacityProduct, message).toBeGreaterThan(0);
    expect(measured.chainVisible, message).toBe(true);
    expect(measured.fontSize, message).toBeGreaterThanOrEqual(12);
    expect(measured.overlapsRows, message).toBe(false);
    expect(measured.footerAboveNav, message).toBe(true);
  } finally {
    try {
      const pending: Route | null = held;
      held = null;
      if (pending) await pending.abort();
    } catch {}
    try {
      await page.unroute("**/api/v1/recordings/*/playback-sources**", hold);
    } catch {}
  }
});
