import type { Schema } from "../../../shared/schema";
import { useCallback, useEffect, useState } from "react";
import { errorMessage } from "../../../shared/format";
import { log } from "../../../shared/log";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { metadataPreferencesApi } from "../api/metadataPreferencesApi";

export type AutomaticLookup = keyof Schema<"UpdateAutomaticLookups">;

export type AutomaticLookups = Record<AutomaticLookup, boolean>;

type State = {
  account: string;
  choices: AutomaticLookups | null;
  saving: boolean;
  error: string;
};

function choices(settings: Schema<"MetadataSettings">): AutomaticLookups {
  return {
    automatic_artist_lookup: settings.automatic_artist_lookup,
    automatic_artwork: settings.automatic_artwork,
    automatic_service_search: settings.automatic_service_search,
  };
}

/**
 * The account's automation choices are stored by the server. Until they are read, nothing counts as
 * enabled, so a failed read never turns a lookup on. A change sends only its own field.
 */
export function useAutomaticLookups(account: string) {
  const scope = useRequestScope(account);
  const [state, setState] = useState<State>({
    account,
    choices: null,
    saving: false,
    error: "",
  });
  const current: State =
    state.account === account
      ? state
      : { account, choices: null, saving: false, error: "" };

  useEffect(() => {
    const request = scope.capture();
    void metadataPreferencesApi
      .settings(account, request.signal)
      .then((settings) => {
        if (request.active())
          setState({
            account,
            choices: choices(settings),
            saving: false,
            error: "",
          });
      })
      .catch((error: unknown) => {
        if (request.active())
          setState({
            account,
            choices: null,
            saving: false,
            error: errorMessage(error),
          });
      });
  }, [account, scope]);

  const change = useCallback(
    async (key: AutomaticLookup, value: boolean) => {
      const request = scope.capture();
      if (!current.choices || current.saving) return;
      setState({ ...current, saving: true, error: "" });
      try {
        const saved = await metadataPreferencesApi.updateAutomation(
          account,
          { [key]: value },
          request.signal,
        );
        if (request.active())
          setState({
            account,
            choices: choices(saved),
            saving: false,
            error: "",
          });
      } catch (error) {
        log.debug("metadata_automation.failed", { action: key });
        if (request.active())
          setState({ ...current, saving: false, error: errorMessage(error) });
      }
    },
    [account, current, scope],
  );

  return {
    loaded: current.choices !== null,
    choices: current.choices,
    saving: current.saving,
    error: current.error,
    change,
  };
}
