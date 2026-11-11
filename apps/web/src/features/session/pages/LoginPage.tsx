import type { Schema } from "../../../shared/schema";
import AuthPage from "../components/AuthPage";
import SetupForm from "../components/SetupForm";
import SignInForm from "../components/SignInForm";
import { useSignIn } from "../hooks/useSignIn";

export default function LoginPage({
  needs_setup: setup,
}: Schema<"LoginPageProps">) {
  const flow = useSignIn(setup);
  return (
    <AuthPage>
      {setup ? <SetupForm {...flow} /> : <SignInForm {...flow} />}
    </AuthPage>
  );
}
