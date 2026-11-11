import type { Schema } from "../../../shared/schema";
import type { RefObject } from "react";

import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import ArtistPhoto from "./ArtistPhoto";
import ArtistFacts from "./ArtistFacts";

export default function ArtistReviewDialog({
  credit,
  profile,
  photos,
  pending,
  opener,
  onConfirm,
  onClose,
}: {
  credit: string;
  profile: Schema<"CatalogArtistProfile">;
  photos: boolean;
  pending: boolean;
  opener: RefObject<HTMLElement | null>;
  onConfirm(): void;
  onClose(): void;
}) {
  return (
    <Dialog
      wide
      title="Review artist"
      description="Save this artist review for this track. Your saved details and playlists are kept."
      busy={pending}
      returnFocus={opener}
      onClose={onClose}
    >
      <div className="review-compare">
        <div
          className="review-side review-credit"
          role="group"
          aria-label="Saved track"
        >
          <h4>Saved track credit</h4>
          <p className="review-title">{credit}</p>
        </div>
        <div className="review-side" role="group" aria-label="Provider profile">
          <h4>Provider profile</h4>
          <span className="review-photo">
            <ArtistPhoto
              artist={profile.artist.id}
              identity={profile.artist.id}
              name={profile.artist.name}
              enabled={photos}
            />
          </span>
          <ArtistFacts profile={profile} />
        </div>
      </div>
      <footer className="review-actions">
        <Button disabled={pending} onClick={onClose}>
          Cancel
        </Button>
        <Button variant="primary" disabled={pending} onClick={onConfirm}>
          {pending ? "Saving artist review…" : "Use this artist"}
        </Button>
      </footer>
    </Dialog>
  );
}
