import { useState, type FormEvent } from "react";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";

export default function AccountDialog({
  pending,
  onCreate,
  onClose,
}: {
  pending: boolean;
  onCreate: (username: string, password: string) => Promise<string | null>;
  onClose: () => void;
}) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (pending) return;
    const failure = await onCreate(username, password);
    if (failure === null) onClose();
    else setError(failure);
  }
  return (
    <Dialog title="Create account" busy={pending} onClose={onClose}>
      <p className="muted">
        Accounts are created here by an administrator. There is no public
        sign-up.
      </p>
      <form className="dialog-form" onSubmit={submit}>
        <label>
          Username
          <input
            autoComplete="off"
            value={username}
            maxLength={64}
            required
            onChange={(event) => setUsername(event.target.value)}
          />
        </label>
        <label>
          Password
          <input
            type="password"
            autoComplete="new-password"
            minLength={12}
            value={password}
            required
            onChange={(event) => setPassword(event.target.value)}
          />
        </label>
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        <div className="dialog-actions">
          <Button disabled={pending} onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" disabled={pending}>
            Create account
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
