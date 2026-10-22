import { vi } from "vitest";

export function browserSessionImages() {
  const created: HTMLImageElement[] = [];
  const loads = new WeakMap<
    HTMLImageElement,
    Set<EventListenerOrEventListenerObject>
  >();
  function size(image: HTMLImageElement, width = 320, height = 320) {
    Object.defineProperty(image, "naturalWidth", {
      configurable: true,
      value: width,
    });
    Object.defineProperty(image, "naturalHeight", {
      configurable: true,
      value: height,
    });
  }
  vi.stubGlobal("Image", function () {
    const image = document.createElement("img");
    created.push(image);
    const listeners = new Set<EventListenerOrEventListenerObject>();
    loads.set(image, listeners);
    const add = image.addEventListener.bind(image);
    vi.spyOn(image, "addEventListener").mockImplementation(
      (name, listener, options) => {
        if (name === "load" && listener) listeners.add(listener);
        add(name, listener, options);
      },
    );
    return image;
  });
  return {
    created,
    size,
    load(image: HTMLImageElement, width = 320, height = 320) {
      size(image, width, height);
      image.dispatchEvent(new Event("load"));
    },
    holdLoad(image: HTMLImageElement) {
      const listeners = [...(loads.get(image) ?? [])];
      const onload = image.onload;
      return () => {
        const event = new Event("load");
        for (const listener of listeners) {
          if (typeof listener === "function") listener.call(image, event);
          else listener.handleEvent(event);
        }
        onload?.call(image, event);
      };
    },
  };
}
