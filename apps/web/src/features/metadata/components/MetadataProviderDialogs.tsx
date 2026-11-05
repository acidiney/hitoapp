import { useState } from "react";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import Listbox from "../../../shared/ui/Listbox";

export type ProviderChoice = { key: string; label: string };

export function AddProviderDialog({
  choices,
  onAdd,
  onClose,
}: {
  choices: ProviderChoice[];
  onAdd(key: string): void;
  onClose(): void;
}) {
  const [chosen, setChosen] = useState(choices[0]?.key ?? "");
  return (
    <Dialog
      title="Add a metadata provider"
      description="Providers are asked in priority order. File tags and your personal edits are always kept."
      onClose={onClose}
    >
      <div className="provider-dialog-field">
        <Listbox
          label="Metadata provider"
          value={chosen}
          options={choices.map((choice) => ({
            value: choice.key,
            label: choice.label,
          }))}
          onChange={setChosen}
        />
      </div>
      <div className="dialog-actions provider-dialog-actions">
        <Button onClick={onClose}>Cancel</Button>
        <Button
          variant="primary"
          disabled={!chosen}
          onClick={() => {
            onAdd(chosen);
            onClose();
          }}
        >
          Add metadata provider
        </Button>
      </div>
    </Dialog>
  );
}

export function RemoveProviderDialog({
  label,
  onRemove,
  onClose,
}: {
  label: string;
  onRemove(): void;
  onClose(): void;
}) {
  return (
    <Dialog
      title={`Remove ${label}?`}
      description="It stops being asked for artist profiles once you save. You can add it again later."
      onClose={onClose}
    >
      <div className="dialog-actions provider-dialog-actions">
        <Button onClick={onClose}>Cancel</Button>
        <Button
          variant="destructive"
          onClick={() => {
            onRemove();
            onClose();
          }}
        >
          Remove provider
        </Button>
      </div>
    </Dialog>
  );
}
