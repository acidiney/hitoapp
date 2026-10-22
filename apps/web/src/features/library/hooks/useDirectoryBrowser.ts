import { useEffect, useState } from "react";
import { storageApi } from "../api/storageApi";
import { errorMessage } from "../../../shared/format";
import type {
  DirectoryPage,
  DirectoryRef,
  SharedDirectory,
} from "../../../shared/types";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";

export function useDirectoryBrowser(account: string) {
  const [directories, setDirectories] = useState<SharedDirectory[]>([]);
  const [selection, select] = useState<DirectoryRef | null>(null);
  const scope = useRequestScope(`${account}\0${JSON.stringify(selection)}`);
  const [page, setPage] = useState<{
    selection: DirectoryRef;
    data: DirectoryPage;
  } | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [revision, retry] = useState(0);

  useEffect(() => {
    const request = scope.capture();
    const controller = new AbortController();
    const signal = AbortSignal.any([request.signal, controller.signal]);
    setLoading(true);
    setError("");
    const operation = selection
      ? storageApi.browse(account, selection, null, signal).then((data) => {
          if (request.active() && !signal.aborted) setPage({ selection, data });
        })
      : storageApi.directories(account, signal).then((result) => {
          if (request.active() && !signal.aborted) setDirectories(result);
        });
    void operation
      .catch((failure) => {
        if (request.active() && !signal.aborted)
          setError(errorMessage(failure));
      })
      .finally(() => {
        if (request.active() && !signal.aborted) setLoading(false);
      });
    return () => controller.abort();
  }, [account, scope, selection, revision]);

  const current = page?.selection === selection ? page.data : null;
  async function more() {
    if (!selection || loading || current?.next_offset == null) return;
    const request = scope.capture();
    setLoading(true);
    setError("");
    try {
      const data = await storageApi.browse(
        account,
        selection,
        current.next_offset,
        request.signal,
      );
      if (request.active())
        setPage((previous) =>
          previous?.selection === selection
            ? {
                selection,
                data: {
                  ...data,
                  directories: [
                    ...previous.data.directories,
                    ...data.directories,
                  ],
                },
              }
            : previous,
        );
    } catch (failure) {
      if (request.active()) setError(errorMessage(failure));
    } finally {
      if (request.active()) setLoading(false);
    }
  }
  return {
    directories,
    selection,
    select,
    page: current,
    loading,
    error,
    more,
    retry: () => retry((value) => value + 1),
  };
}
