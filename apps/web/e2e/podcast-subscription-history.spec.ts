import { expect, test, type Request } from "@playwright/test";
import type { Page as InertiaPage } from "@inertiajs/core";
import type { components } from "../src/generated";
import { liveConsolePrivacy, livePassword } from "./fixtures/podcast-live";
import {
  podcastHistoryFeed,
  podcastHistoryJson,
  podcastHistoryReader,
} from "./fixtures/podcast-subscription-history";
import { openLibrarySection } from "./fixtures/library-sections";

type Schema = components["schemas"];
test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_PODCAST_HISTORY_E2E !== "1",
  "Requires explicit history opt-in and a fresh packaged disposable server",
);

test("a packaged subscription survives returning to the original empty Inertia history page", async ({
  page,
  context,
  baseURL,
}) => {
  test.setTimeout(150_000);
  page.setDefaultTimeout(10_000);
  page.setDefaultNavigationTimeout(15_000);
  const counts = {
    browser_requests: 0,
    document_requests: 0,
    subscription_posts: 0,
    history_list_reads: 0,
    history_page_reads: 0,
    foreign_requests: 0,
    audio_requests: 0,
    download_requests: 0,
    other_writes: 0,
    page_errors: 0,
  };
  const evidence = {
    initial_props_empty: false,
    receipt_matches: false,
    initial_api_current: false,
    initial_card_visible: false,
    history_api_current: false,
    history_card_visible: false,
    document_unchanged: false,
  };
  const returningRequests = new Set<Request>();
  let returning = false;
  let account = "";
  let subscriptionId = "";
  let stage = "configuration";
  let failed = false;
  let cleanupFailed = false;
  const reader = podcastHistoryReader(page);
  const privacy = await liveConsolePrivacy(context);
  page.on("pageerror", () => counts.page_errors++);
  try {
    const feed = podcastHistoryFeed();
    privacy.forbid(feed);
    expect(baseURL === "http://localhost:8080").toBe(true);
    const origin = new URL(baseURL!).origin;
    await context.route("**/*", async (route) => {
      const request = route.request();
      const target = new URL(request.url());
      const headers = request.headers();
      counts.browser_requests++;
      if (request.resourceType() === "document") counts.document_requests++;
      if (target.origin !== origin) counts.foreign_requests++;
      if (
        request.resourceType() === "media" ||
        /\/audio$/.test(target.pathname)
      )
        counts.audio_requests++;
      if (/\/download(?:\/|$)/.test(target.pathname))
        counts.download_requests++;
      if (request.method() === "POST" && target.pathname === "/api/v1/podcasts")
        counts.subscription_posts++;
      if (
        !["GET", "HEAD"].includes(request.method()) &&
        !(
          request.method() === "POST" &&
          ["/api/v1/setup", "/api/v1/podcasts"].includes(target.pathname)
        )
      )
        counts.other_writes++;
      if (returning && request.method() === "GET") {
        if (target.pathname === "/api/v1/podcasts") {
          counts.history_list_reads++;
          if (
            headers["x-hito-request"] === "1" &&
            headers["x-hito-account"] === account
          )
            returningRequests.add(request);
        }
        if (target.pathname === "/podcasts") counts.history_page_reads++;
      }
      if (
        target.origin !== origin ||
        counts.browser_requests > 160 ||
        counts.audio_requests > 0 ||
        counts.download_requests > 0 ||
        counts.other_writes > 0
      )
        await route.abort();
      else await route.continue();
    });
    stage = "fresh disposable setup";
    expect(
      (await reader.get<Schema["SetupStatus"]>("/api/v1/setup")).needs_setup,
    ).toBe(true);
    await page.goto("/");
    await page.getByLabel("Username", { exact: true }).fill("history-admin");
    await page.getByLabel("Password", { exact: true }).fill(livePassword);
    await page
      .getByRole("button", { name: "Create administrator", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    expect(
      await page.evaluate(
        () => window.isSecureContext && typeof crypto.subtle !== "undefined",
      ),
    ).toBe(true);
    const documentOrigin = await page.evaluate(() => performance.timeOrigin);
    const documentRequests = counts.document_requests;
    const navigation = page.getByRole("navigation", {
      name: "Main navigation",
    });

    stage = "initial Inertia podcast props";
    const [initialResponse] = await Promise.all([
      page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname === "/podcasts" &&
          response.request().method() === "GET" &&
          response.headers()["x-inertia"] === "true",
      ),
      openLibrarySection(page, "Podcasts"),
    ]);
    const initialPage =
      await podcastHistoryJson<InertiaPage<Schema["PodcastIndexPageProps"]>>(
        initialResponse,
      );
    evidence.initial_props_empty =
      initialPage.component === "PodcastIndexPage" &&
      Array.isArray(initialPage.props.subscriptions) &&
      initialPage.props.subscriptions.length === 0;
    expect(evidence.initial_props_empty).toBe(true);
    account = initialPage.props.user.id;
    expect(typeof account === "string" && account.length > 0).toBe(true);
    await expect(
      page.getByRole("heading", {
        name: "No podcast subscriptions yet",
        exact: true,
      }),
    ).toBeVisible();

    stage = "public subscription acceptance";
    await page
      .getByRole("button", { name: "Subscribe to a podcast", exact: true })
      .click();
    await page.getByLabel("Feed address", { exact: true }).fill(feed);
    const [accepted] = await Promise.all([
      page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname === "/api/v1/podcasts" &&
          response.request().method() === "POST",
        { timeout: 15_000 },
      ),
      page.getByRole("button", { name: "Subscribe", exact: true }).click(),
    ]);
    const receipt = await podcastHistoryJson<
      Schema["PodcastSubscriptionReceipt"]
    >(accepted, 202);
    const submitted: Schema["SubscribePodcast"] = accepted
      .request()
      .postDataJSON();
    evidence.receipt_matches =
      receipt.request_id === submitted.request_id &&
      submitted.feed_url === feed &&
      /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(
        receipt.subscription_id,
      );
    expect(evidence.receipt_matches).toBe(true);
    subscriptionId = receipt.subscription_id;
    stage = "authoritative imported subscription";
    let subscription: Schema["PodcastSubscription"] | undefined;
    await expect
      .poll(
        async () => {
          const subscriptions =
            await reader.get<Schema["PodcastSubscription"][]>(
              "/api/v1/podcasts",
            );
          expect(subscriptions.length).toBe(1);
          subscription = subscriptions.find(
            (item) => item.subscription_id === subscriptionId,
          );
          if (subscription?.refresh_failed)
            throw new Error("The public feed check failed");
          return (
            !!subscription &&
            !subscription.refresh_pending &&
            subscription.last_refreshed_at != null &&
            subscription.content_revision > 0
          );
        },
        { timeout: 60_000, intervals: [1_000] },
      )
      .toBe(true);
    evidence.initial_api_current = true;
    privacy.forbid(subscription?.title);
    const podcast = page.locator(`a[href="/podcasts/${subscriptionId}"]`);
    await expect(podcast).toBeVisible();
    await expect(
      page.getByText("Last checked", { exact: false }),
    ).toBeVisible();
    await expect(page.getByText("Checking feed…", { exact: true })).toHaveCount(
      0,
    );
    evidence.initial_card_visible = true;
    expect(counts.subscription_posts).toBe(1);

    stage = "SPA library navigation";
    const [libraryResponse] = await Promise.all([
      page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname === "/library" &&
          response.request().method() === "GET" &&
          response.headers()["x-inertia"] === "true",
      ),
      navigation.getByRole("link", { name: "Library", exact: true }).click(),
    ]);
    await podcastHistoryJson<unknown>(libraryResponse);
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    expect(counts.document_requests).toBe(documentRequests);

    stage = "history owned list response";
    returning = true;
    const [currentResponse] = await Promise.all([
      page.waitForResponse(
        (response) => returningRequests.has(response.request()),
        { timeout: 12_000 },
      ),
      page.goBack({ waitUntil: "commit", timeout: 15_000 }),
    ]);
    const current =
      await podcastHistoryJson<Schema["PodcastSubscription"][]>(
        currentResponse,
      );
    evidence.history_api_current =
      current.length === 1 &&
      current[0].subscription_id === subscriptionId &&
      !current[0].refresh_pending &&
      !current[0].refresh_failed &&
      current[0].content_revision > 0;
    expect(evidence.history_api_current).toBe(true);
    stage = "history current card visibility";
    await expect(
      page.getByRole("heading", { name: "Podcasts", exact: true }),
    ).toBeVisible();
    await expect(podcast).toBeVisible();
    evidence.history_card_visible = true;
    evidence.document_unchanged = await page.evaluate(
      (value) => performance.timeOrigin === value,
      documentOrigin,
    );
    expect(evidence.document_unchanged).toBe(true);
    expect(counts.document_requests).toBe(documentRequests);
    expect(counts.history_page_reads).toBe(0);
    expect(counts.history_list_reads).toBe(1);
    expect(counts.subscription_posts).toBe(1);
    stage = "read-only history privacy checks";
    expect(
      counts.audio_requests +
        counts.download_requests +
        counts.foreign_requests +
        counts.other_writes +
        counts.page_errors,
    ).toBe(0);
    expect(await privacy.clean()).toBe(true);
  } catch {
    failed = true;
    try {
      if (returning && subscriptionId)
        evidence.history_card_visible = await page
          .locator(`a[href="/podcasts/${subscriptionId}"]`)
          .isVisible();
    } catch {}
  } finally {
    try {
      await page.goto("about:blank", { timeout: 5_000 });
    } catch {
      cleanupFailed = true;
    }
  }
  if (failed || cleanupFailed)
    throw new Error(
      `Podcast history verification failed at ${stage}; counts=${JSON.stringify(counts)}; evidence=${JSON.stringify(evidence)}; public_reads=${reader.count()}; cleanup_failed=${cleanupFailed}`,
    );
});
