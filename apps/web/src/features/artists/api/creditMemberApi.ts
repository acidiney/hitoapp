import type { Schema } from "../../../shared/schema";
import { requestJson } from "../../../shared/http";

const part = encodeURIComponent;
const base = (recording: string, artist: string) =>
  `/recordings/${part(recording)}/credit-members/${part(artist)}`;

export const creditMemberApi = {
  context(
    account: string,
    recording: string,
    artist: string,
    body: Schema<"BeginArtistReview">,
    signal: AbortSignal,
  ) {
    return requestJson<Schema<"CreditMemberReviewContext">>(
      `${base(recording, artist)}/artist-review/context`,
      {
        action: "credit_member_review.context",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  search(
    account: string,
    recording: string,
    artist: string,
    body: Schema<"SearchCreditMemberCandidates">,
    signal: AbortSignal,
  ) {
    return requestJson<Schema<"CreditMemberCandidateSearchAvailability">>(
      `${base(recording, artist)}/artist-review/search`,
      {
        action: "credit_member_review.search",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  preview(
    account: string,
    recording: string,
    artist: string,
    body: Schema<"SelectCreditMemberCandidate">,
    signal: AbortSignal,
  ) {
    return requestJson<Schema<"CreditMemberCandidatePreviewAvailability">>(
      `${base(recording, artist)}/artist-review/preview`,
      {
        action: "credit_member_review.preview",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  confirm(
    account: string,
    recording: string,
    artist: string,
    body: Schema<"SelectCreditMemberCandidate">,
    signal: AbortSignal,
  ) {
    return requestJson<Schema<"ArtistAssociation">>(
      `${base(recording, artist)}/artist-review/confirm`,
      {
        action: "credit_member_review.confirm",
        method: "POST",
        account,
        body,
        signal,
      },
    );
  },
  choice(
    account: string,
    recording: string,
    artist: string,
    signal: AbortSignal,
  ) {
    return requestJson<Schema<"RecordingCreditMemberIdentityChoice">>(
      `${base(recording, artist)}/artist-identity`,
      { action: "credit_member_identity.read", account, signal },
    );
  },
  choose(
    account: string,
    recording: string,
    artist: string,
    body: Schema<"ChooseCreditMemberIdentity">,
    signal: AbortSignal,
  ) {
    return requestJson<Schema<"CreditMemberIdentityChoiceReceipt">>(
      `${base(recording, artist)}/artist-identity`,
      {
        action: "credit_member_identity.choose",
        method: "PUT",
        account,
        body,
        signal,
      },
    );
  },
};
