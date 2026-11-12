import { useId, useState, type FormEvent } from "react";
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
  const id = useId();
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
      <form className="dialog-form" onSubmit={submit}>
        <div className="dialog-field">
          <label htmlFor={`${id}-username`}>Username</label>
          <input
            id={`${id}-username`}
            autoComplete="off"
            value={username}
            maxLength={64}
            required
            onChange={(event) => setUsername(event.target.value)}
          />
        </div>
        <div className="dialog-field">
          <label htmlFor={`${id}-password`}>Password</label>
          <input
            id={`${id}-password`}
            type="password"
            autoComplete="new-password"
            minLength={12}
            value={password}
            required
            onChange={(event) => setPassword(event.target.value)}
          />
          <small>
            At least 12 characters. They can change it after signing in.
          </small>
        </div>
        <p className="dialog-note">
          New accounts are members. Give access to a library afterwards.
        </p>
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
            disabled={pending || username === "" || password.length < 12}
          >
            Create account
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
