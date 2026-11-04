import { useSyncExternalStore } from "react";
import {
  getConnection,
  getLostChange,
  subscribeConnection,
} from "../connection";

export function useConnection() {
  return useSyncExternalStore(subscribeConnection, getConnection);
}

export function useLostChange() {
  return useSyncExternalStore(subscribeConnection, getLostChange);
}
