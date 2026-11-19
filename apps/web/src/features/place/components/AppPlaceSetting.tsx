import { useState } from "react";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import SettingRow from "../../../shared/ui/SettingRow";
import { useAppPlace } from "../hooks/useAppPlace";

export default function AppPlaceSetting({ account }: { account: string }) {
  const { offer, busy, change } = useAppPlace(account);
  const [asking, setAsking] = useState(false);
  if (!offer?.changeable) return null;
  return (
    <>
      <SettingRow
        title="This app uses"
        hint="This computer"
        action={<Button onClick={() => setAsking(true)}>Change…</Button>}
      />
      {asking && (
        <Dialog
          title="Use a different place?"
          busy={busy}
          onClose={() => setAsking(false)}
        >
          <p>
            The music stops and the first screen opens. Your library on this
            computer is kept.
          </p>
          <div className="dialog-actions">
            <Button onClick={() => setAsking(false)} disabled={busy}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={() => void change()}
              disabled={busy}
            >
              Change
            </Button>
          </div>
        </Dialog>
      )}
    </>
  );
}
