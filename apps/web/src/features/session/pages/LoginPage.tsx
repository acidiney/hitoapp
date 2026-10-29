import AuthIntro from "../components/AuthIntro";
import SetupForm from "../components/SetupForm";
import SignInForm from "../components/SignInForm";
import { useSignIn } from "../hooks/useSignIn";
import type { LoginPageProps } from "../../../shared/types";

export default function LoginPage({ needs_setup: setup }: LoginPageProps) {
  const flow = useSignIn(setup);
  return (
    <main className="auth-page">
      {setup ? (
        <AuthIntro
          decorative
          tone="action"
          headline="Welcome to your server."
          detail="After you create the account, you choose your music folders."
        />
      ) : (
        <AuthIntro
          decorative
          headline={
            <>
              Your music.
              <br />
              Your server.
            </>
          }
        />
      )}
      <div className="auth-form" data-mode={setup ? "setup" : "sign-in"}>
        {setup ? <SetupForm {...flow} /> : <SignInForm {...flow} />}
      </div>
    </main>
  );
}
