export type ViewportBox = {
  innerHeight: number;
  height: number;
  offsetTop: number;
};

export function keyboardInset({
  innerHeight,
  height,
  offsetTop,
}: ViewportBox): number {
  return Math.max(0, Math.round(innerHeight - height - offsetTop));
}

let locks = 0;

export function lockPageScroll(): () => void {
  locks += 1;
  document.documentElement.setAttribute("data-dialog-open", "");
  return () => {
    locks -= 1;
    if (locks <= 0)
      document.documentElement.removeAttribute("data-dialog-open");
  };
}

export function followKeyboard(dialog: HTMLElement): () => void {
  const viewport = window.visualViewport;
  const phone = window.matchMedia?.("(max-width: 767px)").matches ?? false;
  if (!viewport || !phone) return () => {};
  const apply = () => {
    const inset = keyboardInset({
      innerHeight: window.innerHeight,
      height: viewport.height,
      offsetTop: viewport.offsetTop,
    });
    dialog.style.setProperty("--sheet-bottom", `${inset}px`);
    dialog.style.setProperty("--sheet-height", `${viewport.height}px`);
  };
  const reveal = (event: FocusEvent) => {
    const target = event.target;
    if (!(target instanceof HTMLElement)) return;
    window.setTimeout(() => target.scrollIntoView({ block: "center" }), 300);
  };
  apply();
  viewport.addEventListener("resize", apply);
  viewport.addEventListener("scroll", apply);
  dialog.addEventListener("focusin", reveal);
  return () => {
    viewport.removeEventListener("resize", apply);
    viewport.removeEventListener("scroll", apply);
    dialog.removeEventListener("focusin", reveal);
  };
}
