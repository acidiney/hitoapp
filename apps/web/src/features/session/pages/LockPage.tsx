import BrandMark from "../../../shared/ui/BrandMark";
import type { LockPageProps } from "../../../shared/types";
import { NoticeBanner, NoticeProvider } from "../../../shared/ui/Notices";
import { useSignOut } from "../hooks/useSignOut";
import { useUnlock } from "../hooks/useUnlock";

export default function LockPage(props: LockPageProps) {
  return (
    <NoticeProvider>
      <NoticeBanner />
      <LockScreen {...props} />
    </NoticeProvider>
  );
}

function LockScreen({ username, account_id }: LockPageProps) {
  const { pin, setPin, busy, error, submit } = useUnlock(account_id);
  const signOut = useSignOut(account_id, () => {});
  return (
    <main className="auth-page">
      <div className="auth-intro">
        <div className="brand">
          <BrandMark />
          <span className="brand-word">hito</span>
        </div>
        <p>Enter your PIN to carry on where you left off.</p>
      </div>
      <div className="auth-form">
        <form onSubmit={submit}>
          <h2>Locked</h2>
          <strong>{username}</strong>
          <label>
            PIN
            <input
              value={pin}
              type="password"
              inputMode="numeric"
              autoComplete="off"
              maxLength={8}
              autoFocus
              required
              onChange={(event) => setPin(event.target.value)}
            />
          </label>
          {error && (
            <p className="form-error" role="alert">
              {error}
            </p>
          )}
          <button className="button primary" disabled={busy}>
            {busy ? "Checking…" : "Unlock"}
          </button>
          <button
            className="button"
            type="button"
            disabled={signOut.busy}
            onClick={() => void signOut.signOut()}
          >
            Sign out
          </button>
        </form>
      </div>
    </main>
  );
}
