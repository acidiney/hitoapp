import { useSyncExternalStore } from "react";
import { getConnection, subscribeConnection } from "../connection";

export function useConnection() {
  return useSyncExternalStore(subscribeConnection, getConnection);
}
