import { useEffect, useRef, useState } from "react";
import { storageApi } from "../api/storageApi";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";
import type {
  DirectoryRef,
  HostFolderOffer,
  MusicFolder,
} from "../../../shared/types";

const none: HostFolderOffer = { picker: false, suggestion: null };

function covered(suggestion: DirectoryRef, folders: MusicFolder[]) {
  const wanted = suggestion.components ?? [];
  return folders.some((folder) => {
    const have = folder.directory?.components ?? [];
    if (folder.directory?.directory_id !== suggestion.directory_id)
      return false;
    const shared = Math.min(have.length, wanted.length);
    return have.slice(0, shared).every((part, index) => part === wanted[index]);
  });
}

export function useHostFolders(
  account: string,
  folders: MusicFolder[],
  ready: boolean,
  connect: (directory: DirectoryRef) => Promise<boolean>,
) {
  const [offer, setOffer] = useState(none);
  const [dismissed, dismiss] = useState(false);
  const [choosing, setChoosing] = useState(false);
  const [error, setError] = useState("");
  const lifetime = useRef(new AbortController());
  useEffect(() => {
    const controller = new AbortController();
    lifetime.current = controller;
    storageApi
      .hostOffer(account, controller.signal)
      .then((value) => {
        if (!controller.signal.aborted) setOffer(value ?? none);
      })
      .catch(() => {
        if (!controller.signal.aborted) setOffer(none);
      });
    return () => controller.abort();
  }, [account]);
  const suggestion =
    ready &&
    !dismissed &&
    offer.suggestion &&
    !covered(offer.suggestion, folders)
      ? offer.suggestion
      : null;
  async function choose() {
    const signal = lifetime.current.signal;
    setChoosing(true);
    setError("");
    try {
      const choice = await storageApi.chooseHostFolder(account, signal);
      if (signal.aborted) return;
      log.info("folders.host_chosen", {
        account_id: account,
        action: choice.directory ? "chosen" : "cancelled",
      });
      if (choice.directory) await connect(choice.directory);
    } catch (failure) {
      if (!signal.aborted) setError(errorMessage(failure));
    } finally {
      if (!signal.aborted) setChoosing(false);
    }
  }
  return {
    picker: offer.picker,
    suggestion,
    choosing,
    error,
    choose,
    dismiss: () => dismiss(true),
    accept: async () => {
      if (suggestion && !choosing && (await connect(suggestion))) dismiss(true);
    },
  };
}
