import { chooseMusicService } from "./music-service";
import { expect, type Locator, type Page } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import { reviewPluginInstallation } from "./plugin-installation-review";

export class SpotifyYouTubeLiveFailure extends Error {}

export async function setUpSpotifyYouTubeLiveAccount(page: Page) {
  try {
    await page.goto("/library");
    const username = page.getByLabel("Username", { exact: true });
    const password = page.getByLabel("Password", { exact: true });
    await username.waitFor({ state: "visible" });
    await password.waitFor({ state: "visible" });
    const setup = page.getByRole("button", {
      name: "Create administrator",
      exact: true,
    });
    if (!(await setup.isVisible()))
      throw new SpotifyYouTubeLiveFailure(
        "The Spotify to YouTube live test requires an uninitialized disposable server",
      );
    await username.fill("admin");
    await password.fill("docker browser test password");
    await setup.click();
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
  } catch (error) {
    if (error instanceof SpotifyYouTubeLiveFailure) throw error;
    throw new SpotifyYouTubeLiveFailure(
      "The disposable live account initialization failed",
    );
  }
}

export function spotifyYouTubeConfiguration() {
  const rawPlaylist = process.env.HITO_SPOTIFY_TEST_PLAYLIST;
  const rawYouTube = process.env.HITO_YOUTUBE_LIVE_BASE_URL;
  if (!rawPlaylist || !rawYouTube)
    throw new SpotifyYouTubeLiveFailure(
      "The opted-in test requires its isolated YouTube service and public Spotify playlist configuration",
    );
  let playlist: URL;
  let youtube: URL;
  try {
    playlist = new URL(rawPlaylist);
    youtube = new URL(rawYouTube);
  } catch {
    throw new SpotifyYouTubeLiveFailure("The live configuration is invalid");
  }
  if (
    playlist.protocol !== "https:" ||
    playlist.hostname !== "open.spotify.com" ||
    playlist.port ||
    playlist.username ||
    playlist.password ||
    playlist.hash ||
    !/^\/playlist\/[A-Za-z0-9]{22}$/.test(playlist.pathname) ||
    youtube.protocol !== "http:" ||
    youtube.hostname !== "plugin-youtube" ||
    youtube.port !== "8093" ||
    youtube.pathname !== "/" ||
    youtube.username ||
    youtube.password ||
    youtube.search ||
    youtube.hash
  )
    throw new SpotifyYouTubeLiveFailure(
      "The test requires the approved isolated YouTube alias and a public Spotify playlist link",
    );
  playlist.search = "";
  return {
    spotify: "http://plugin-spotify:8092/",
    youtube: youtube.href,
    playlist: playlist.href,
  };
}

export async function connectLiveProvider(
  page: Page,
  address: string,
  name: string,
  offered: (service: Schema<"PluginService">) => void,
) {
  await page
    .getByRole("link", { name: "Account and settings", exact: true })
    .click();
  await page
    .getByRole("navigation", { name: "Settings sections" })
    .getByRole("link", { name: "Plugins", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Your plugins", exact: true }),
  ).toBeVisible();
  if (!(await page.getByLabel("Service address", { exact: true }).isVisible()))
    await page.getByText("Services for this server", { exact: true }).click();
  await expect(page.getByLabel("Service name", { exact: true })).toHaveCount(0);
  await expect(
    page.getByLabel("Custom label (optional)", { exact: true }),
  ).toBeHidden();
  await page.getByLabel("Service address", { exact: true }).fill(address);
  const offerResponse = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === "/api/v1/plugins/services" &&
      response.request().method() === "POST",
  );
  await page
    .getByRole("button", { name: "Offer service", exact: true })
    .click();
  const offer = await offerResponse;
  if (offer.status() !== 201)
    throw new SpotifyYouTubeLiveFailure(
      `Service offer returned HTTP ${offer.status()}`,
    );
  const body: Schema<"OfferPluginService"> = offer.request().postDataJSON();
  expect(Object.keys(body).join(",") === "base_url").toBe(true);
  expect(body.base_url === address).toBe(true);
  const service: Schema<"PluginService"> = await offer.json();
  offered(service);
  expect(service.name === name).toBe(true);
  await page
    .getByRole("combobox", { name: "Service", exact: true })
    .selectOption(service.id);
  await expect(
    page.getByLabel("Bearer token (optional)", { exact: true }),
  ).toHaveValue("");
  const reviewed = await reviewPluginInstallation(page, service, name);
  const installResponse = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === "/api/v1/plugins" &&
      response.request().method() === "POST",
  );
  await page
    .getByRole("button", { name: "Accept and connect", exact: true })
    .click();
  const installed = await installResponse;
  if (installed.status() !== 201)
    throw new SpotifyYouTubeLiveFailure(
      `Personal connection returned HTTP ${installed.status()}`,
    );
  expect(installed.request().postDataJSON().bearer_token === null).toBe(true);
  const accepted: Schema<"InstallPlugin"> = installed.request().postDataJSON();
  expect(accepted.accept_manifest_digest === reviewed.body.digest).toBe(true);
  const connection: Schema<"PluginInstallation"> = await installed.json();
  expect(!connection.credential_set && connection.manifest.name === name).toBe(
    true,
  );
  return connection;
}

function normalized(value: string) {
  return value
    .normalize("NFKD")
    .replace(/\p{M}/gu, "")
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim();
}

export function conservativeCandidate(
  original: Schema<"MetadataObservation">,
  items: Schema<"ProviderRecording">[],
) {
  const title = normalized(original.title);
  const artist = normalized(original.artist)
    .split(" ")
    .filter((word) => word.length >= 2);
  const variants = [
    "cover",
    "karaoke",
    "remix",
    "live",
    "instrumental",
    "sped up",
    "slowed",
    "nightcore",
    "reaction",
    "8d",
  ];
  if (
    title.length < 3 ||
    !artist.length ||
    original.duration_ms <= 10_000 ||
    original.duration_ms > 600_000
  )
    throw new SpotifyYouTubeLiveFailure(
      "The imported track needs bounded, identifiable metadata for explicit review",
    );
  const ranked = items
    .map((candidate, index) => ({ candidate, index }))
    .filter(({ candidate }) => {
      const candidateTitle = normalized(candidate.title);
      const combined = `${candidateTitle} ${normalized(candidate.artist)}`;
      return (
        /^[A-Za-z0-9_-]{11}$/.test(candidate.id) &&
        ` ${candidateTitle} `.includes(` ${title} `) &&
        artist.every((word) => combined.split(" ").includes(word)) &&
        candidate.duration_ms != null &&
        candidate.duration_ms > 10_000 &&
        candidate.duration_ms <= 600_000 &&
        Math.abs(candidate.duration_ms - original.duration_ms) <=
          Math.max(10_000, original.duration_ms * 0.08) &&
        variants.every(
          (variant) =>
            !` ${candidateTitle} `.includes(` ${variant} `) ||
            ` ${title} `.includes(` ${variant} `),
        )
      );
    })
    .sort(
      (left, right) =>
        Math.abs(left.candidate.duration_ms! - original.duration_ms) -
        Math.abs(right.candidate.duration_ms! - original.duration_ms),
    );
  if (!ranked.length)
    throw new SpotifyYouTubeLiveFailure(
      "No conservative candidate was found; an explicit human match decision is required",
    );
  return { ...ranked[0], qualifying: ranked.length };
}

export async function reviewLiveCandidate(
  page: Page,
  {
    panel,
    result,
    recording,
    selection,
    original,
    personalTitle,
    personalArtist,
  }: {
    panel: Locator;
    result: Locator;
    recording: string;
    selection: { candidate: Schema<"ProviderRecording"> };
    original: Schema<"MetadataObservation">;
    personalTitle: string;
    personalArtist: string;
  },
) {
  const reviewedResponse = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname ===
        `/api/v1/recordings/${recording}/sources/review` &&
      response.request().method() === "POST",
  );
  await result
    .getByRole("button", { name: "Review match", exact: true })
    .click();
  const reviewed = await reviewedResponse;
  if (reviewed.status() !== 200)
    throw new SpotifyYouTubeLiveFailure(
      `Source review returned HTTP ${reviewed.status()}`,
    );
  const review: Schema<"ProviderSourceReview"> = await reviewed.json();
  expect(review.candidate.id === selection.candidate.id).toBe(true);
  conservativeCandidate(original, [review.candidate]);
  const comparison = panel.getByRole("dialog", {
    name: "Review source match",
    exact: true,
  });
  await expect(
    comparison.getByRole("heading", { name: "Saved track", exact: true }),
  ).toBeVisible();
  await expect(
    comparison.getByRole("heading", {
      name: "Candidate source",
      exact: true,
    }),
  ).toBeVisible();
  expect(
    await comparison
      .getByRole("group", { name: "Saved track", exact: true })
      .evaluate(
        (element, saved) =>
          element.textContent?.includes(saved.title) &&
          element.textContent?.includes(saved.artist),
        { title: personalTitle, artist: personalArtist },
      ),
  ).toBe(true);
  expect(
    await comparison
      .getByRole("group", { name: "Candidate source", exact: true })
      .evaluate(
        (element, candidate) =>
          element.textContent?.includes(candidate.title) &&
          element.textContent?.includes(candidate.artist),
        { title: review.candidate.title, artist: review.candidate.artist },
      ),
  ).toBe(true);
  return { review, comparison };
}

export async function searchLiveCandidate(
  page: Page,
  {
    youtube,
    original,
  }: {
    youtube: Schema<"PluginInstallation">;
    original: Schema<"MetadataObservation">;
  },
) {
  const panel = page.getByRole("region", {
    name: /^(Find audio|Add another source)$/,
  });
  await panel
    .getByRole("button", { name: "Find a source", exact: true })
    .click();
  await chooseMusicService(page, panel, youtube.id);
  const query = `${original.title} ${original.artist}`.trim();
  if (query.length > 512)
    throw new SpotifyYouTubeLiveFailure(
      "The original metadata exceeds the bounded search input",
    );
  await panel
    .getByRole("searchbox", { name: "Search music", exact: true })
    .fill(query);
  const searchedResponse = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname ===
        `/api/v1/plugins/${youtube.id}/search` &&
      response.request().method() === "POST",
  );
  await panel.getByRole("button", { name: "Search", exact: true }).click();
  const searched = await searchedResponse;
  if (searched.status() !== 200)
    throw new SpotifyYouTubeLiveFailure(
      `Source search returned HTTP ${searched.status()}`,
    );
  const results: Schema<"SearchPage"> = await searched.json();
  const selection = conservativeCandidate(original, results.items);
  const result = panel
    .getByRole("list", { name: "Source search results" })
    .locator(":scope > li")
    .nth(selection.index);
  await expect(result.getByRole("heading")).toBeVisible();
  expect(
    await result
      .getByRole("heading")
      .evaluate(
        (element, title) => element.textContent === title,
        selection.candidate.title,
      ),
  ).toBe(true);
  return { panel, results, selection, result };
}
