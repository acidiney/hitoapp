import { useState } from "react";
import { router } from "@inertiajs/react";
import { errorMessage } from "../../../shared/format";
import { useNotice } from "../../../shared/ui/Notices";
import { forgetCatalog } from "../../artists/api/catalogCache";
import { sessionApi } from "../api/sessionApi";

export function useSignOut(account: string, onSignedOut: () => void) {
  const notice = useNotice();
  const [busy, setBusy] = useState(false);
  async function signOut() {
    setBusy(true);
    try {
      await sessionApi.signOut(account);
      forgetCatalog();
      router.clearHistory();
      onSignedOut();
      window.location.replace("/");
    } catch (error) {
      notice(errorMessage(error));
      setBusy(false);
    }
  }
  return { busy, signOut };
}
