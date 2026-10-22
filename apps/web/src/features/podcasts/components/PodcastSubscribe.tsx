import { useRef, useState, type FormEvent } from "react";

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
  const [address, setAddress] = useState("");
  const draftRevision = useRef(0);
  function submit(event: FormEvent) {
    event.preventDefault();
    const submittedRevision = draftRevision.current;
    void subscribe(address).then((accepted) => {
      if (accepted && draftRevision.current === submittedRevision)
        setAddress("");
    });
  }
  return (
    <section className="playlist-import" aria-label="Subscribe to a podcast">
      <h2>Subscribe to a podcast</h2>
      <p className="muted">
        Add an RSS feed to see its episodes. Audio plays when you choose an
        episode.
      </p>
      <form className="playlist-import-form" onSubmit={submit}>
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
        <button
          className="button primary"
          type="submit"
          disabled={pending || hasAttempt || !address.trim()}
        >
          {pending ? "Subscribing…" : "Subscribe"}
        </button>
        {error && <p role="alert">{error}</p>}
        {hasAttempt && !pending && (
          <div className="playlist-import-recovery">
            <p className="muted">
              Retry recovers the original subscription request, including its
              feed address.
            </p>
            <button
              className="button"
              type="button"
              onClick={() => void retry()}
            >
              Retry subscription
            </button>
          </div>
        )}
      </form>
    </section>
  );
}
