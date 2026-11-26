// What a link says in its tooltip: where a click takes it, and the address it points at. Every link the
// window draws asks this one place (docs/devel/links.md), so a link whose words name another place still
// shows where it really goes, and #474 and #475 change the words here once a click learns to go elsewhere.

/**
 * The tooltip of a link to `href`. The address comes from the link's `href`, never from its words, and shows
 * whole: the native tooltip wraps a long line, and an ellipsis in the middle could hide the owner or
 * repository of a long `github.com` path. Holding `Ctrl` leaves the tooltip as it is, since a native tooltip
 * does not update while it shows.
 *
 * For now every link follows to the system browser (#474 and #475 route some clicks later).
 */
export function linkTooltip(href: string): string {
  return `Open in the browser: ${href}`;
}
