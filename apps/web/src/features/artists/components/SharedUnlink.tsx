import type { Schema } from "../../../shared/schema";
import type { useSharedUnlink } from "../hooks/useSharedUnlink";

import Button from "../../../shared/ui/Button";

export default function SharedUnlink({
  state,
  sharing,
}: {
  state: Schema<"ArtistAssociationChanges"> | null;
  sharing: ReturnType<typeof useSharedUnlink>;
}) {
  if (
    !state ||
    (!state.shareable &&
      !state.shared &&
      state.retired.length === 0 &&
      state.shared_hidden === 0 &&
      !sharing.notice &&
      !sharing.error)
  )
    return null;
  const busy = sharing.pending !== null || sharing.retrying;
  return (
    <section aria-label="Shared artist review corrections">
      {state.retired.length > 0 && (
        <p>
          {state.retired.length}{" "}
          {state.retired.length === 1
            ? "unlink no longer applies"
            : "unlinks no longer apply"}{" "}
          because the file&apos;s artist tag changed. They keep hiding their
          reviews.
        </p>
      )}
      {state.shared_hidden > 0 && (
        <p>
          {state.shared_hidden}{" "}
          {state.shared_hidden === 1 ? "saved review is" : "saved reviews are"}{" "}
          hidden by a correction an administrator shared with every account.
          Review the artist again to keep your own choice.
        </p>
      )}
      {state.shareable && (
        <Button
          type="button"
          disabled={busy}
          onClick={() => void sharing.share()}
        >
          Apply this unlink for everyone
        </Button>
      )}
      {state.shared && (
        <>
          <p>This unlink applies to every account.</p>
          <Button
            type="button"
            disabled={busy}
            onClick={() => void sharing.withdraw()}
          >
            Withdraw the shared unlink
          </Button>
        </>
      )}
      {sharing.error && <p role="alert">{sharing.error}</p>}
      {sharing.retrying && (
        <Button type="button" onClick={() => void sharing.retry()}>
          Retry the same shared unlink request
        </Button>
      )}
      {sharing.notice && <p role="status">{sharing.notice}</p>}
    </section>
  );
}
