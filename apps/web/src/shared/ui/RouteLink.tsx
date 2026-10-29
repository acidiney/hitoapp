import type { ComponentProps, MouseEvent } from "react";
import { router } from "@inertiajs/react";
import { log } from "../log";

/** Moves focus to the new page's heading once the router has swapped the page in. */
export function focusPageHeading() {
  window.setTimeout(() => {
    const heading = document.querySelector<HTMLElement>("main h1");
    if (!heading) return;
    heading.tabIndex = -1;
    heading.focus({ preventScroll: true });
  }, 0);
}

export function visitFromPlayer(href: string) {
  router.visit(href, { onSuccess: focusPageHeading });
}

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
    visitFromPlayer(href);
  }
  return <a {...rest} href={href} onClick={follow} />;
}
