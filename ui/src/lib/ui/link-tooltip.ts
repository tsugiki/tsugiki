// What a link says in its tooltip, and what a click follows: every link the window draws asks this one place
// (docs/devel/links.md). A link's words never decide either, so a link whose text names another place still
// shows and goes where it really points, and #474 and #475 change the rule here once a click learns to go
// elsewhere.

/** What a click follows: a link, or an image map's area. */
export const LINK_SELECTOR = "a, area";

/** Whether a click sends the address to the link handler. */
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

/** Decodes a fragment after the browser's literal ID lookup. */
function decodedFragment(fragment: string): string {
  try {
    return decodeURIComponent(fragment);
  } catch {
    return fragment;
  }
}

function scrollToFragment(e: MouseEvent, link: Element, scope: HTMLElement, fragment: string): boolean {
  const elements = Array.from(scope.querySelectorAll("[id]"));
  const decoded = decodedFragment(fragment);
  const target =
    elements.find((element) => element.id === fragment) ?? elements.find((element) => element.id === decoded);
  if (!target) return false;

  // The browser looks across comments and can open a new window for a modified click or link target.
  const first = document.getElementById(fragment) ?? document.getElementById(decoded);
  const sameWindow = ["", "_self"].includes((link.getAttribute("target") ?? "").toLowerCase());
  if (
    first !== target ||
    !sameWindow ||
    link.hasAttribute("download") ||
    e.ctrlKey ||
    e.metaKey ||
    e.shiftKey ||
    e.altKey
  ) {
    e.preventDefault();
    target.scrollIntoView();
  }
  return true;
}

/** Follows the link `e` clicked, allowing fragments only inside the supplied rendered text. */
export function followLink(e: MouseEvent, onlink: (address: string) => void, scope?: HTMLElement): void {
  // Only an element takes a click.
  const link = (e.target as Element).closest(LINK_SELECTOR);
  if (!link) return;
  const address = link.getAttribute("href");
  if (
    scope &&
    address?.startsWith("#") &&
    address.length > 1 &&
    scrollToFragment(e, link, scope, address.slice(1))
  )
    return;
  e.preventDefault();
  if (follows(address)) onlink(address);
}
