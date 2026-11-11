import { useEffect, useRef } from "react";
import type { Schema } from "../../../shared/schema";
import AuthPage from "../components/AuthPage";
import { AuthAlert, AuthField, DotSpinner } from "../components/AuthFields";
import { NoticeBanner, NoticeProvider } from "../../../shared/ui/Notices";
import { useSignOut } from "../hooks/useSignOut";
import { useUnlock } from "../hooks/useUnlock";
import Button from "../../../shared/ui/Button";

export default function LockPage(props: Schema<"LockPageProps">) {
  return (
    <NoticeProvider>
      <NoticeBanner />
      <LockScreen {...props} />
    </NoticeProvider>
  );
}

function LockScreen({ username, account_id }: Schema<"LockPageProps">) {
  const { pin, setPin, busy, error, submit } = useUnlock(account_id);
  const signOut = useSignOut(account_id, () => {});
  const form = useRef<HTMLFormElement>(null);
  useEffect(() => {
    if (error) form.current?.querySelector<HTMLInputElement>("input")?.focus();
  }, [error]);
  return (
    <AuthPage>
      <form
        ref={form}
        className="auth-form"
        aria-label="Unlock Hito"
        onSubmit={submit}
      >
        <div className="auth-heading">
          <h1>Locked</h1>
        </div>
        <div className="auth-account">
          <span className="auth-avatar" aria-hidden="true">
            {username.slice(0, 1).toUpperCase()}
          </span>
          <span className="auth-account-text">
            <strong>{username}</strong>
            <span>Music stopped when this locked</span>
          </span>
        </div>
        <AuthField
          label="PIN"
          value={pin}
          onValue={setPin}
          type="password"
          inputMode="numeric"
          autoComplete="off"
          maxLength={8}
          autoFocus
          required
          pin
          disabled={busy}
          hint="4 to 8 digits. After 5 wrong tries you sign in with your password."
          hintId="pin-hint"
        />
        {error && <AuthAlert>{error}</AuthAlert>}
        <Button variant="action" type="submit" disabled={busy}>
          {busy ? (
            <>
              <DotSpinner />
              Checking…
            </>
          ) : (
            "Unlock"
          )}
        </Button>
        <button
          type="button"
          className="auth-text-button"
          disabled={signOut.busy}
          onClick={() => void signOut.signOut()}
        >
          Sign out
        </button>
      </form>
    </AuthPage>
  );
}
