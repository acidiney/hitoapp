import { expect, type Page } from "@playwright/test";

export async function measureNotice(page: Page) {
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

export async function seedMusicFolder(page: Page) {
  await seedFolder(page, "Music");
}

export async function seedFolder(page: Page, folder: "Music" | "Archive") {
  await page.goto("/settings?section=music");
  const connected = page
    .getByRole("list", { name: "Connected music folders" })
    .getByRole("listitem")
    .filter({ has: page.getByRole("heading", { name: folder, exact: true }) });
  if ((await connected.count()) === 0) {
    await page
      .getByRole("button", { name: "Add music folder", exact: true })
      .click();
    const picker = page.getByRole("dialog", {
      name: "Add a music folder",
      exact: true,
    });
    await picker
      .getByRole("button", { name: `Open ${folder}`, exact: true })
      .click();
    await picker
      .getByRole("button", { name: "Use this folder", exact: true })
      .click();
    await expect(picker).toBeHidden();
  }
  await expect(
    connected.filter({ hasText: "1 file read · read-only" }),
  ).toHaveCount(1, { timeout: 20_000 });
}

export async function expectReadableNotice(page: Page, phone: boolean) {
  const measured = await measureNotice(page);
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
  if (phone) expect(measured.footerAboveNav, message).toBe(true);
  return measured;
}
