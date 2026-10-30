import { useAccountSwitch } from "../../../shared/hooks/useAccountSwitch";

/** Whether a search with few library results is also sent to the connected services. */
export function useAutomaticServiceSearch(account: string) {
  return useAccountSwitch("search-services-auto", account);
}
