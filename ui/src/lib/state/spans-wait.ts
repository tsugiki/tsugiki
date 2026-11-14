// How rows wait for the spans that highlight them (#178, #455): the diff's rows and the Definition peek's
// lines ask the core for both at once, and show once the spans come or a moment has passed.

/** How long rows wait for their spans before they show plain. The spans repaint the rows when they come. */
export const SPANS_WAIT = 50;

/** Starts the wait for `spans` as their rows are asked for. The function it gives, called once the rows have
 * come, gives the spans if they came by then or within `SPANS_WAIT` of the ask, and otherwise nothing: the
 * rows then show plain until `spans` comes. */
export function waitForSpans<S>(spans: Promise<S | undefined>): () => Promise<S | undefined> {
  const waited = new Promise<undefined>((resolve) => setTimeout(resolve, SPANS_WAIT));
  return () => Promise.race([spans, waited]);
}
