// What a link says in its tooltip, and what a click follows: every link the window draws asks this one place
// (docs/devel/links.md). A link's words never decide either, so a link whose text names another place still
// shows and goes where it really points, and #474 and #475 change the rule here once a click learns to go
// elsewhere.

/** What a click follows: a link, or an image map's area. */
export const LINK_SELECTOR = "a, area";

/** Whether a click follows the link at `address`. A link within the text (`#…`) moves to a heading in a page
 * Tsugiki doesn't show whole, and one with nowhere to go has no address. */
function follows(address: string | null): address is string {
  if (!address) return false;
  return !address.startsWith("#");
}

/**
 * The tooltip of a link to `href`. The address comes from the link's `href`, never from its words, and shows
 * whole: the native tooltip wraps a long line, and an ellipsis in the middle could hide the owner or
 * repository of a long `github.com` path. Holding `Ctrl` leaves the tooltip as it is, since a native tooltip
 * does not update while it shows.
 *
 * For now every link a click follows goes to the system browser (#474 and #475 route some clicks later).
 */
export function linkTooltip(href: string): string {
  return `Open in the browser: ${href}`;
}

/** The tooltip a link to `href` shows, or `undefined` where a click doesn't follow it and it says nothing. */
export function linkTitle(href: string | null): string | undefined {
  return follows(href) ? linkTooltip(href) : undefined;
}

/** Follows the link `e` clicked in the system browser, if a click follows it, and keeps the window where it
 * is. Enter on a link arrives as a click, so keys need nothing more. */
export function followLink(e: MouseEvent, onlink: (address: string) => void): void {
  // Only an element takes a click.
  const link = (e.target as Element).closest(LINK_SELECTOR);
  if (!link) return;
  e.preventDefault();
  const address = link.getAttribute("href");
  if (follows(address)) onlink(address);
}
