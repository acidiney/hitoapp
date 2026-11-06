import { isDeepStrictEqual } from "node:util";
import { expect, type Page, type Route } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import { preferencesAction } from "./metadata-preferences-state";
import type { GroupReader } from "./artist-identity-groups-state";
import type { ReviewedGroupMember } from "./artist-identity-groups-review";

export async function chooseGroupMember(
  page: Page,
  read: GroupReader,
  reviewed: ReviewedGroupMember,
  loseAcknowledgement = false,
) {
  const id = reviewed.member.recording.id;
  const path = `/api/v1/recordings/${id}/artist-identity`;
  await page.goto(`/recordings/${id}`);
  const artist = page.getByRole("region", {
    name: "Linked artist",
    exact: true,
  });
  await artist
    .getByRole("button", { name: "Find artist information", exact: true })
    .click();
  await artist
    .getByRole("button", { name: "View saved artist review 1", exact: true })
    .click();
  const choice = artist.getByRole("region", {
    name: "Artist choice for this track",
    exact: true,
  });
  const choose = choice.getByRole("button", {
    name: "Choose this reviewed artist",
    exact: true,
  });
  await expect(choose).toBeEnabled();
  let receipt: Schema<"ArtistIdentityChoiceReceipt"> | null = null;
  let body: Schema<"ChooseRecordingArtistIdentity"> | null = null;
  let lostCurrent: Schema<"RecordingArtistIdentityChoice"> | null = null;
  let invalid = false;
  const attempts: string[] = [];
  const intercept = async (route: Route) => {
    if (route.request().method() !== "PUT") return route.continue();
    const raw = route.request().postData();
    if (!raw || Buffer.byteLength(raw) > 16_384 || attempts.length >= 2) {
      invalid = true;
      return route.abort("failed");
    }
    attempts.push(raw);
    if (attempts.length === 2) return route.continue();
    try {
      const response = await route.fetch({ timeout: 10_000 });
      try {
        const bytes = await response.body();
        if (response.status() !== 200 || bytes.length > 4096)
          throw new Error("Invalid owned acknowledgement");
        receipt = JSON.parse(
          bytes.toString(),
        ) as Schema<"ArtistIdentityChoiceReceipt">;
        body = JSON.parse(raw) as Schema<"ChooseRecordingArtistIdentity">;
        lostCurrent = await read<Schema<"RecordingArtistIdentityChoice">>(path);
      } finally {
        await response.dispose();
      }
    } catch {
      invalid = true;
    }
    await route.abort("failed");
  };
  if (loseAcknowledgement) await page.route(`**${path}`, intercept);
  try {
    if (loseAcknowledgement) {
      await choose.click();
      await expect(choice.getByRole("alert")).toContainText(
        "Retry the same request",
      );
      await expect(
        choice.getByRole("link", { name: "Open chosen artist", exact: true }),
      ).toHaveCount(0);
      const accepted =
        lostCurrent as Schema<"RecordingArtistIdentityChoice"> | null;
      expect(
        !invalid &&
          accepted?.revision === 1 &&
          accepted.state.status === "selected",
      ).toBe(true);
      const retried = await preferencesAction<
        Schema<"ArtistIdentityChoiceReceipt">
      >(
        page,
        choice.getByRole("button", {
          name: "Retry the same artist choice request",
          exact: true,
        }),
        "PUT",
        path,
        200,
      );
      expect(
        attempts.length === 2 &&
          attempts[0] === attempts[1] &&
          isDeepStrictEqual(retried.body, receipt) &&
          isDeepStrictEqual(retried.sent, body),
      ).toBe(true);
    } else {
      const chosen = await preferencesAction<
        Schema<"ArtistIdentityChoiceReceipt">
      >(page, choose, "PUT", path, 200);
      receipt = chosen.body;
      body = chosen.sent as Schema<"ChooseRecordingArtistIdentity">;
    }
    const sent = body as Schema<"ChooseRecordingArtistIdentity"> | null;
    const acknowledgement =
      receipt as Schema<"ArtistIdentityChoiceReceipt"> | null;
    expect(
      !invalid &&
        !!sent &&
        !!acknowledgement &&
        sent.expected_revision === 0 &&
        sent.association_id === reviewed.association.id &&
        isDeepStrictEqual(sent.context, reviewed.context) &&
        acknowledgement.recording_id === id &&
        acknowledgement.request_id === sent.request_id &&
        acknowledgement.revision === 1 &&
        acknowledgement.selection?.association_id === reviewed.association.id,
    ).toBe(true);
    await expect(
      choice.getByRole("link", { name: "Open chosen artist", exact: true }),
    ).toBeVisible();
    await expect(choice).toHaveAttribute("aria-busy", "false");
    const current = await read<Schema<"RecordingArtistIdentityChoice">>(path);
    if (current.state.status !== "selected")
      throw new Error("The current artist choice is missing");
    expect(
      current.revision === 1 &&
        current.recording_id === id &&
        current.state.association_id === reviewed.association.id &&
        current.state.group_id === acknowledgement?.selection?.group_id,
    ).toBe(true);
    await expect(
      choice.getByRole("link", { name: "Open chosen artist", exact: true }),
    ).toHaveAttribute("href", `/artist-groups/${current.state.group_id}`);
    return {
      ...reviewed,
      groupId: current.state.group_id,
      request: sent!,
      receipt: acknowledgement!,
    };
  } finally {
    if (loseAcknowledgement) await page.unroute(`**${path}`, intercept);
  }
}

export type ChosenGroupMember = Awaited<ReturnType<typeof chooseGroupMember>>;

export async function resetGroupMember(
  page: Page,
  read: GroupReader,
  chosen: ChosenGroupMember,
) {
  const id = chosen.member.recording.id;
  await page.goto(`/recordings/${id}`);
  const artist = page.getByRole("region", {
    name: "Linked artist",
    exact: true,
  });
  await artist
    .getByRole("button", { name: "Find artist information", exact: true })
    .click();
  const choice = artist.getByRole("region", {
    name: "Artist choice for this track",
    exact: true,
  });
  await expect(
    choice.getByRole("link", { name: "Open chosen artist", exact: true }),
  ).toBeVisible();
  const reset = await preferencesAction<Schema<"ArtistIdentityChoiceReceipt">>(
    page,
    choice.getByRole("button", {
      name: "Reset artist choice for this track",
      exact: true,
    }),
    "POST",
    `/api/v1/recordings/${id}/artist-identity/reset`,
    200,
  );
  const sent = reset.sent as Schema<"ResetRecordingArtistIdentity">;
  expect(
    sent.expected_revision === 1 &&
      reset.body?.request_id === sent.request_id &&
      reset.body.recording_id === id &&
      reset.body.revision === 2 &&
      reset.body.selection === null,
  ).toBe(true);
  await expect(
    choice.getByText("No reviewed artist selected for this track.", {
      exact: true,
    }),
  ).toBeVisible();
  await expect(choice).toHaveAttribute("aria-busy", "false");
  await expect(
    choice.getByRole("link", { name: "Open chosen artist", exact: true }),
  ).toHaveCount(0);
  expect(
    isDeepStrictEqual(await read(`/api/v1/recordings/${id}/artist-identity`), {
      recording_id: id,
      revision: 2,
      state: { status: "unselected" },
    }),
  ).toBe(true);
}
