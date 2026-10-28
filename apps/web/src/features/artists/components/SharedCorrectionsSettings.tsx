import { useSharedCorrections } from "../hooks/useSharedCorrections";

export default function SharedCorrectionsSettings({
  account,
}: {
  account: string;
}) {
  const corrections = useSharedCorrections(account);
  const { page } = corrections;
  const busy = corrections.pending !== null || corrections.retrying;
  return (
    <section aria-labelledby="shared-corrections-title">
      <h2 id="shared-corrections-title">Shared artist corrections</h2>
      <p>
        Corrections an administrator applied for every account. Withdrawing one
        restores the reviews it hid; accounts that reviewed the recording again
        keep their own choice.
      </p>
      {corrections.failed && (
        <p role="alert">Hito could not read the shared corrections.</p>
      )}
      {page && page.items.length === 0 && (
        <p>No corrections are shared with every account.</p>
      )}
      {page && page.items.length > 0 && (
        <ul>
          {page.items.map((item) => (
            <li key={item.id}>
              {item.kind === "unlink" ? "Unlink" : "Split"} ·{" "}
              {item.recordings} {item.recordings === 1 ? "recording" : "recordings"}
              {item.retired > 0 &&
                ` · ${item.retired} no longer ${item.retired === 1 ? "matches" : "match"} the file tags`}
              {" · "}
              {item.applied ? "Applied" : "Withdrawn"}{" "}
              <button
                type="button"
                className="button"
                disabled={busy}
                onClick={() => void corrections.toggle(item)}
              >
                {item.applied ? "Withdraw" : "Restore"}
                {" shared correction "}
                {page.items.indexOf(item) + 1 + page.offset}
              </button>
            </li>
          ))}
        </ul>
      )}
      {page && page.total > page.limit && (
        <nav aria-label="Shared correction pages">
          <button
            type="button"
            className="button"
            disabled={page.offset === 0}
            onClick={corrections.previous}
          >
            Previous
          </button>
          <button
            type="button"
            className="button"
            disabled={page.offset + page.limit >= page.total}
            onClick={corrections.next}
          >
            Next
          </button>
        </nav>
      )}
      {corrections.error && <p role="alert">{corrections.error}</p>}
      {corrections.retrying && (
        <button
          type="button"
          className="button"
          onClick={() => void corrections.retry()}
        >
          Retry the same shared correction request
        </button>
      )}
      {corrections.notice && <p role="status">{corrections.notice}</p>}
    </section>
  );
}
