import type { Schema } from "../../../shared/schema";
import AuthPage from "../components/AuthPage";
import SetupForm from "../components/SetupForm";
import SignInForm from "../components/SignInForm";
import { useSignIn } from "../hooks/useSignIn";
import { useAppServer } from "../../place/hooks/useAppServer";

export default function LoginPage({
  needs_setup: setup,
}: Schema<"LoginPageProps">) {
  const flow = useSignIn(setup);
  const app = useAppServer();
  return (
    <AuthPage>
      {flow.setup ? (
        <SetupForm {...flow} app={app} />
      ) : (
        <SignInForm {...flow} app={app} />
      )}
    </AuthPage>
  );
}
