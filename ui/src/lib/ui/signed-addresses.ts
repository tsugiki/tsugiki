// GitHub's rendered HTML gives each newer upload an address on `private-user-images` with a signed query,
// good for five minutes and signed afresh on each fetch (#761). The open Issue or PR is fetched about once a
// minute, so its HTML changes each time in those queries alone. Drawing it afresh would close a section the
// user opened, drop the selection and load each image again, so the view keeps what it drew and takes only
// the fresh signatures.

/** The signed query of an upload's address. Its characters can't end an attribute or a tag, so two HTMLs
 * that differ only in these parse to the same nodes. */
const SIGNATURE = /(?<=https:\/\/private-user-images\.githubusercontent\.com\/[^\s"'<>?]*)\?[^\s"'<>]*/;

/** Whether `one` and `other` differ only in the signatures of their uploads' addresses. */
export const sameButSignatures = (one: string, other: string): boolean => {
  const ones = one.split(SIGNATURE);
  const others = other.split(SIGNATURE);
  return ones.length === others.length && ones.every((piece, i) => piece === others[i]);
};

/** Whether `element` is an image, or a source of a picture whose image, that keeps the address it has: one
 * still loading, or one that has drawn. Only an image that failed, such as one whose address had run out,
 * takes a fresh one. */
const keepsAddress = (element: Element): boolean => {
  const image = element instanceof HTMLSourceElement ? element.parentElement?.querySelector("img") : element;
  return image instanceof HTMLImageElement && !(image.complete && image.naturalWidth === 0);
};

/** The elements and text under `root`, in document order. */
const nodesUnder = (root: Node): Node[] => {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
  const nodes: Node[] = [];
  while (walker.nextNode()) nodes.push(walker.currentNode);
  return nodes;
};

/** Gives the elements under `drawn` the signed addresses of those under `fresh`, the nodes of HTML that
 * differs from what `drawn` drew only in its signatures. A link takes the fresh address, and so does an
 * image that failed. An image that has drawn keeps its address, and goes on showing without loading again,
 * and so does one still loading. Gives `false`, and changes nothing, unless the two hold the same elements
 * and the same text, since a signature in the text, or an element added since, needs drawing afresh. */
export const resign = (drawn: Node, fresh: Node): boolean => {
  const shown = nodesUnder(drawn);
  const wanted = nodesUnder(fresh);
  if (shown.length !== wanted.length) return false;
  const changes: (() => void)[] = [];
  for (const [i, node] of wanted.entries()) {
    const target = shown[i] as Node;
    if (target.nodeName !== node.nodeName) return false;
    if (node.nodeType === Node.TEXT_NODE) {
      if (node.textContent !== target.textContent) return false;
      continue;
    }
    // The walk shows only elements and text.
    const element = target as Element;
    if (keepsAddress(element)) continue;
    for (const { name, value } of (node as Element).attributes) {
      if (SIGNATURE.test(value) && element.getAttribute(name) !== value) {
        changes.push(() => {
          element.setAttribute(name, value);
        });
      }
    }
  }
  for (const change of changes) change();
  return true;
};
