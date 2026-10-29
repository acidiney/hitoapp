import type { ComponentProps, MouseEvent } from "react";
import { router } from "@inertiajs/react";
import { log } from "../log";

export default function RouteLink({
  href,
  onNavigate,
  onClick,
  ...rest
}: ComponentProps<"a"> & { href: string; onNavigate?: () => void }) {
  function follow(event: MouseEvent<HTMLAnchorElement>) {
    onClick?.(event);
    if (
      event.defaultPrevented ||
      event.button !== 0 ||
      event.metaKey ||
      event.ctrlKey ||
      event.shiftKey ||
      event.altKey
    )
      return;
    event.preventDefault();
    log.info("navigation.requested", { route: href.split("/")[1] ?? "" });
    onNavigate?.();
    router.visit(href);
  }
  return <a {...rest} href={href} onClick={follow} />;
}
