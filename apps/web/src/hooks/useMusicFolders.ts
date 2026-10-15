import { useEffect, useRef, useState } from "react";
import { storageApi } from "../storageApi";
import { libraryApi } from "../api";
import { errorMessage } from "../format";
import type { DirectoryRef, MusicFolder } from "../types";
import { useRequestScope } from "./useRequestScope";
import { log } from "../log";

export function useMusicFolders(account: string, library: string) {
  const scope = useRequestScope(`${account}\0${library}`);
  const [state, setState] = useState<{
    scope: typeof scope;
    folders: MusicFolder[];
    error: string;
    loading: boolean;
  }>({ scope, folders: [], error: "", loading: true });
  const [revision, refresh] = useState(0);
  const [pending, setPending] = useState<{
    scope: typeof scope;
    action: string;
  } | null>(null);
  const [failure, setFailure] = useState<{
    scope: typeof scope;
    message: string;
  } | null>(null);
  const busy = useRef<typeof scope | null>(null);
  useEffect(() => {
    const request = scope.capture();
    const controller = new AbortController();
    const signal = AbortSignal.any([request.signal, controller.signal]);
    let timer: ReturnType<typeof setTimeout>;
    async function load() {
      try {
        const folders = await storageApi.folders(account, library, signal);
        if (!request.active() || signal.aborted || busy.current === scope)
          return;
        setState({ scope, folders, error: "", loading: false });
        if (
          folders.some(
            (folder) =>
              folder.latest_job &&
              ["queued", "running"].includes(folder.latest_job.state),
          )
        ) {
          timer = setTimeout(() => void load(), 1000);
        }
      } catch (error) {
        if (request.active() && !signal.aborted)
          setState((current) => ({
            scope,
            folders: current.scope === scope ? current.folders : [],
            error: errorMessage(error),
            loading: false,
          }));
      }
    }
    void load();
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  }, [account, library, scope, revision]);

  async function perform(
    action: string,
    operation: (signal: AbortSignal) => Promise<unknown>,
  ) {
    if (busy.current === scope) return false;
    const request = scope.capture();
    busy.current = scope;
    setPending({ scope, action });
    setFailure(null);
    refresh((value) => value + 1);
    try {
      await operation(request.signal);
      if (!request.active()) return false;
      log.info("folders.changed", {
        account_id: account,
        library_id: library,
        action,
      });
      return true;
    } catch (error) {
      if (request.active()) setFailure({ scope, message: errorMessage(error) });
      return false;
    } finally {
      if (busy.current === scope) busy.current = null;
      if (request.active()) {
        setPending(null);
        refresh((value) => value + 1);
      }
    }
  }
  return {
    folders: state.scope === scope ? state.folders : [],
    loading: state.scope !== scope || state.loading,
    error:
      failure?.scope === scope
        ? failure.message
        : state.scope === scope
          ? state.error
          : "",
    pending: pending?.scope === scope ? pending.action : null,
    retry: () => refresh((value) => value + 1),
    connect: (directory: DirectoryRef) =>
      perform("connect", (signal) =>
        storageApi.connect(account, library, directory, signal),
      ),
    remove: (folder: MusicFolder) =>
      perform(folder.id, (signal) =>
        storageApi.remove(account, folder, signal),
      ),
    scan: (folder: MusicFolder) =>
      perform(folder.id, (signal) =>
        storageApi.scan(account, folder.id, signal),
      ),
    scanAll: () =>
      perform("scan-all", (signal) =>
        libraryApi.scan(account, library, signal),
      ),
  };
}
