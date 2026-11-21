import { useId, useState } from "react";
import type { Schema } from "../../../shared/schema";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import { sourceLabels } from "../model/sourceLabels";

type Source = Schema<"Source">;

export default function SameRecordingDialog({
  from,
  candidates,
  onSend,
  onClose,
}: {
  from: Source;
  candidates: Source[];
  onSend(other: string): void;
  onClose(): void;
}) {
  const others = candidates.filter((source) => source.id !== from.id);
  const [other, setOther] = useState(others[0]?.id ?? "");
  const group = useId();
  return (
    <Dialog title="Suggest as same recording" onClose={onClose}>
      <p>
        Suggest that {sourceLabels(from).primary} and the source you pick are
        the same recording. The suggestion goes to the shared community service
        for review.
      </p>
      <fieldset className="same-recording-choices">
        <legend>Same recording as</legend>
        {others.map((source) => (
          <label key={source.id}>
            <input
              type="radio"
              name={group}
              checked={other === source.id}
              onChange={() => setOther(source.id)}
            />{" "}
            {sourceLabels(source).primary}{" "}
            <span className="rd-choice-origin">
              {sourceLabels(source).secondary}
            </span>
          </label>
        ))}
      </fieldset>
      <div className="dialog-actions">
        <Button onClick={onClose}>Cancel</Button>
        <Button
          variant="primary"
          disabled={!other}
          onClick={() => onSend(other)}
        >
          Send suggestion
        </Button>
      </div>
    </Dialog>
  );
}
