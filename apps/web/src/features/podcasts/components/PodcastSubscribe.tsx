import { useRef, useState, type FormEvent } from "react";
import { Plus } from "lucide-react";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";

export default function PodcastSubscribe({
  pending,
  hasAttempt,
  error,
  subscribe,
  retry,
}: {
  pending: boolean;
  hasAttempt: boolean;
  error: string;
  subscribe: (address: string) => Promise<boolean>;
  retry: () => Promise<boolean>;
}) {
  const [open, setOpen] = useState(false);
  const [address, setAddress] = useState("");
  const draftRevision = useRef(0);
  function submit(event: FormEvent) {
    event.preventDefault();
    const submittedRevision = draftRevision.current;
    void subscribe(address).then((accepted) => {
      if (!accepted) return;
      if (draftRevision.current === submittedRevision) setAddress("");
      setOpen(false);
    });
  }
  function retrySubscription() {
    void retry().then((accepted) => {
      if (accepted) setOpen(false);
    });
  }
  return (
    <>
      <Button
        variant="action"
        aria-haspopup="dialog"
        aria-label="Subscribe to a podcast"
        onClick={() => setOpen(true)}
      >
        <Plus size={17} aria-hidden="true" />
        Subscribe
      </Button>
      {open && (
        <Dialog
          title="Subscribe to a podcast"
          busy={pending}
          onClose={() => setOpen(false)}
        >
          <p className="muted">
            Add an RSS feed to see its episodes. Audio plays when you choose an
            episode.
          </p>
          <form className="dialog-form" onSubmit={submit}>
            <label>
              Feed address
              <input
                type="url"
                value={address}
                onChange={(event) => {
                  draftRevision.current += 1;
                  setAddress(event.target.value);
                }}
                autoComplete="off"
                spellCheck={false}
                maxLength={2048}
                required
                placeholder="https://example.com/feed.xml"
              />
            </label>
            {error && <p role="alert">{error}</p>}
            {hasAttempt && !pending && (
              <div className="playlist-import-recovery">
                <p className="muted">
                  Retry recovers the original subscription request, including
                  its feed address.
                </p>
                <Button onClick={retrySubscription}>Retry subscription</Button>
              </div>
            )}
            <div className="dialog-actions">
              <Button disabled={pending} onClick={() => setOpen(false)}>
                Cancel
              </Button>
              <Button
                variant="primary"
                type="submit"
                disabled={pending || hasAttempt || !address.trim()}
              >
                {pending ? "Subscribing…" : "Subscribe"}
              </Button>
            </div>
          </form>
        </Dialog>
      )}
    </>
  );
}
