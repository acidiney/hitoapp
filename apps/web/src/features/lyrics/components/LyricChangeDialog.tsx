import type { RefObject } from "react";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import LyricChoices, { type LyricChoicesProps } from "./LyricChoices";

export default function LyricChangeDialog({
  choices,
  opener,
  onFind,
  onClose,
}: {
  choices: LyricChoicesProps;
  opener: RefObject<HTMLElement | null>;
  onFind: () => void;
  onClose: () => void;
}) {
  return (
    <Dialog
      wide
      title="Lyrics for this track"
      description="Choose where the lyrics come from, remove a choice you no longer want, or find a different match."
      returnFocus={opener}
      onClose={onClose}
    >
      <LyricChoices {...choices} />
      <footer className="review-actions">
        <Button onClick={onFind}>Find lyrics</Button>
        <Button variant="primary" onClick={onClose}>
          Done
        </Button>
      </footer>
    </Dialog>
  );
}
