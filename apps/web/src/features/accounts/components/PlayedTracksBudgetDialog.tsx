import { useEffect, useId, useState, type FormEvent } from "react";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import { automaticCopiesApi } from "../../downloads/api/automaticCopiesApi";
import {
  budgetBytes,
  budgetGib,
  LARGEST_BUDGET_GIB,
} from "../../downloads/model/space";

export default function PlayedTracksBudgetDialog({
  account,
  user,
  name,
  onClose,
}: {
  account: string;
  user: string;
  name: string;
  onClose: () => void;
}) {
  const id = useId();
  const [value, setValue] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    const controller = new AbortController();
    automaticCopiesApi
      .budget(account, user, controller.signal)
      .then((budget) => {
        if (!controller.signal.aborted)
          setValue(budgetGib(budget.budget_bytes));
      })
      .catch((failure: unknown) => {
        if (!controller.signal.aborted) setError(errorMessage(failure));
      });
    return () => controller.abort();
  }, [account, user]);
  const bytes = value === null ? null : budgetBytes(value);
  const invalid = value !== null && bytes === null;
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (pending || bytes === null) return;
    setPending(true);
    setError("");
    const started = performance.now();
    let outcome = "failed";
    try {
      await automaticCopiesApi.setBudget(
        account,
        user,
        bytes,
        AbortSignal.timeout(30_000),
      );
      outcome = "accepted";
      onClose();
    } catch (failure) {
      setError(errorMessage(failure));
    } finally {
      setPending(false);
      log.info("automatic_copies.budget_saved", {
        account_id: user,
        reason: outcome,
        duration_ms: Math.round(performance.now() - started),
      });
    }
  }
  return (
    <Dialog
      title="Space for played tracks"
      description={`How much space Hito may use to keep played tracks for ${name}. The default is 5 GiB. Downloads don't count toward it.`}
      busy={pending}
      onClose={onClose}
    >
      <form className="dialog-form" onSubmit={submit}>
        <div className="dialog-field">
          <label htmlFor={`${id}-space`}>Space in GiB</label>
          <input
            id={`${id}-space`}
            inputMode="decimal"
            autoComplete="off"
            value={value ?? ""}
            disabled={value === null}
            aria-invalid={invalid || undefined}
            aria-describedby={`${id}-space-hint`}
            onChange={(event) => setValue(event.target.value)}
          />
          <small
            id={`${id}-space-hint`}
            className={invalid ? "form-error" : undefined}
          >
            {invalid
              ? `Enter a number from 0 to ${LARGEST_BUDGET_GIB}.`
              : `From 0 to ${LARGEST_BUDGET_GIB} GiB. 0 stops keeping played tracks for this account. A lower amount removes the least played copies right away.`}
          </small>
        </div>
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        <div className="dialog-actions">
          <Button disabled={pending} onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="submit"
            variant="action"
            disabled={pending || bytes === null}
          >
            Save
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
