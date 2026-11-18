import { useState } from "react";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import SettingRow from "../../../shared/ui/SettingRow";
import type { Schema } from "../../../shared/schema";
import { useAppPlace } from "../hooks/useAppPlace";

type Current = Schema<"HostPlaceCurrent">;

function shown(current: Current | null | undefined) {
  return current?.kind === "server" ? current.origin : "This computer";
}

function consequence(current: Current | null | undefined) {
  if (current?.kind !== "server") {
    return "The music stops and the first screen opens. Your library on this computer is kept.";
  }
  const host = new URL(current.origin).host;
  return `The music stops and the first screen opens. You stay signed in to ${host}, and your library on this computer is kept.`;
}

export default function AppPlaceSetting({ account }: { account: string }) {
  const { offer, busy, change } = useAppPlace(account);
  const [asking, setAsking] = useState(false);
  if (!offer?.changeable) return null;
  return (
    <>
      <SettingRow
        title="This app uses"
        hint={shown(offer.current)}
        action={<Button onClick={() => setAsking(true)}>Change…</Button>}
      />
      {asking && (
        <Dialog
          title="Use a different place?"
          busy={busy}
          onClose={() => setAsking(false)}
        >
          <p>{consequence(offer.current)}</p>
          <footer className="review-actions">
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
          </footer>
        </Dialog>
      )}
    </>
  );
}
