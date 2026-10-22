import type { components } from "../../../generated";
import {
  builtin,
  catalog,
  connections,
  reference,
  settings,
} from "../../metadata/fixtures/metadataPreferences";
import { savedRecording, sourceAttachmentPage } from "../../../fixtures/sourceAttachment";

type Schema = components["schemas"];

export const artistReviewAccount = sourceAttachmentPage.user.id;
export const artistReviewDetail = sourceAttachmentPage.detail;
export const artistQuery = "Edited shared artist search";
export const firstCatalog = catalog(101, "Personal metadata catalog");
export const secondCatalog = catalog(102, "Personal metadata catalog");
const unselected = catalog(103, "Unselected metadata catalog");
const audioOnly = catalog(104, "Audio source provider");
audioOnly.installation.manifest.capabilities = ["search", "sources"];
export const artistReviewConnections = connections([
  firstCatalog,
  secondCatalog,
  unselected,
  audioOnly,
]);
artistReviewConnections.services.push({
  id: "10000000-0000-4000-8000-000000000105",
  name: "Offered but unconnected catalog",
  base_url: "https://offered-catalog.invalid/",
  revision: 1,
});
export const artistReviewSettings = settings(
  [builtin, reference(firstCatalog), reference(secondCatalog)],
  7,
);
export const artistReviewContext = {
  account_id: artistReviewAccount,
  recording_id: artistReviewDetail.recording_id,
  library_id: savedRecording.library_id,
  artist_id: "30000000-0000-4000-8000-000000000001",
  credited_name: artistReviewDetail.artist.value,
  source_id: null,
  observation_id: artistReviewDetail.selected_observation!.id,
  artist_operation_id: artistReviewDetail.artist.operation_id,
  access_grant_id: "40000000-0000-4000-8000-000000000001",
  metadata_binding: {
    id: "50000000-0000-4000-8000-000000000001",
    revision: 3,
  },
  provider: reference(secondCatalog),
  policy_revision: artistReviewSettings.revision,
  connection: {
    installation_revision: secondCatalog.installation.revision,
    service_id: secondCatalog.service.id,
    service_revision: secondCatalog.service.revision,
  },
} satisfies Schema["ArtistReviewContext"];

export const artistSearch = {
  query: artistQuery,
  cursor: null,
  snapshot: null,
  limit: 20,
} satisfies Schema["ArtistSearchRequest"];
export const artistSearchResult = {
  status: "available",
  context: artistReviewContext,
  page: {
    query: artistQuery,
    snapshot: "private-provider-snapshot:7",
    total_count: 2,
    next_cursor: null,
    items: [
      {
        artist: {
          id: "opaque:artist/first?credit=shared",
          name: "Shared artist name",
        },
        kind: "group",
        disambiguation: "Electronic ensemble from Lisbon",
      },
      {
        artist: {
          id: "opaque:artist/second?credit=shared~á",
          name: "Shared artist name",
        },
        kind: "person",
        disambiguation: "Acoustic singer from Luanda",
      },
    ],
  },
} satisfies Schema["ArtistCandidateSearchAvailability"];
export const artistPreview = {
  status: "available",
  context: artistReviewContext,
  profile: {
    artist: artistSearchResult.page.items[1].artist,
    kind: "person",
    sort_name: "Artist name, Shared",
    disambiguation: "Acoustic singer from Luanda, established in 2012",
    revision: "private-provider-profile:11",
  },
} satisfies Schema["ArtistCandidatePreviewAvailability"];
export const artistSelection = {
  context: artistReviewContext,
  provider_artist_id: artistPreview.profile.artist.id,
} satisfies Schema["SelectArtistCandidate"];
export const artistAssociation = {
  id: "60000000-0000-4000-8000-000000000001",
  account_id: artistReviewContext.account_id,
  recording_id: artistReviewContext.recording_id,
  library_id: artistReviewContext.library_id,
  artist_id: artistReviewContext.artist_id,
  source_id: null,
  observation_id: artistReviewContext.observation_id,
  role: "recording_artist",
  provider: artistReviewContext.provider,
  provider_artist_id: artistPreview.profile.artist.id,
  evidence: "user_confirmed",
  revision: 1,
} satisfies Schema["ArtistAssociation"];
export const reviewedArtistProfile = {
  status: "available",
  association_id: artistAssociation.id,
  profile: artistPreview.profile,
} satisfies Schema["ArtistProfileAvailability"];
