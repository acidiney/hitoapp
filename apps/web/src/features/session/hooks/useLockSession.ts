import { useState } from "react";
import { errorMessage } from "../../../shared/format";
import { useNotice } from "../../../shared/ui/Notices";
import { lockApi } from "../api/lockApi";
import { notifySessionChanged } from "../session";

export function useLockSession(account: string, stopPlayback: () => void) {
  const notice = useNotice();
  const [busy, setBusy] = useState(false);
  async function lock() {
    setBusy(true);
    stopPlayback();
    try {
      await lockApi.lock(account);
      notifySessionChanged();
      window.location.reload();
    } catch (error) {
      notice(errorMessage(error));
      setBusy(false);
    }
  }
  return { busy, lock };
}
