import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";

export default function DiscardEditsDialog({
  onKeep,
  onDiscard,
}: {
  onKeep: () => void;
  onDiscard: () => void;
}) {
  return (
    <Dialog
      title="Discard your changes?"
      description="The name, order and removals you changed here will go back to how they were."
      onClose={onKeep}
    >
      <div className="dialog-actions">
        <Button onClick={onKeep}>Keep editing</Button>
        <Button variant="destructive" onClick={onDiscard}>
          Discard changes
        </Button>
      </div>
    </Dialog>
  );
}
