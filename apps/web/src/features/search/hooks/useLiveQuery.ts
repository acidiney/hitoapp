import { useEffect, useRef, useState } from "react";
import { router } from "@inertiajs/react";

const SETTLE_MS = 300;

export function useLiveQuery(query: string) {
  const [draft, setDraft] = useState(query);
  const sent = useRef(query);
  const timer = useRef<number | null>(null);
  useEffect(() => {
    if (query !== sent.current) {
      sent.current = query;
      setDraft(query);
    }
  }, [query]);
  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    [],
  );
  function visit(value: string, replace: boolean) {
    sent.current = value;
    router.get("/search", value ? { q: value } : {}, {
      preserveState: true,
      preserveScroll: true,
      replace,
    });
  }
  function change(value: string) {
    setDraft(value);
    if (timer.current !== null) window.clearTimeout(timer.current);
    const trimmed = value.trim();
    if (trimmed === sent.current) return;
    timer.current = window.setTimeout(() => visit(trimmed, true), SETTLE_MS);
  }
  function submit(value: string) {
    if (timer.current !== null) window.clearTimeout(timer.current);
    visit(value.trim(), false);
  }
  return { draft, change, submit };
}
