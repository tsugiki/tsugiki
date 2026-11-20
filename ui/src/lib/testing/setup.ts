// Fills the gaps jsdom leaves, once for every test file (Vitest's `setupFiles`). It assigns the globals
// itself, as a test that calls `vi.unstubAllGlobals()` would undo a `vi.stubGlobal` here. A test that
// stubs one of them with `vi.stubGlobal` gets this one back when it unstubs.

// The lint rules' tests run in Node, which has no DOM to fill in.
if (typeof HTMLElement !== "undefined") {
  // jsdom lays nothing out, so a bind:clientHeight, or a diff that measures its threads, has nothing to
  // observe.
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };

  // jsdom has no `inert`, which a browser reflects to the attribute, as Svelte sets the property.
  Object.defineProperty(HTMLElement.prototype, "inert", {
    configurable: true,
    get(this: HTMLElement) {
      return this.hasAttribute("inert");
    },
    set(this: HTMLElement, inert: boolean) {
      this.toggleAttribute("inert", inert);
    },
  });

  // jsdom has no pointer capture, which a pane's edge takes as a drag starts.
  HTMLElement.prototype.setPointerCapture = () => {};
}
