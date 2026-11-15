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

/** Whether `element` is an image that has drawn, or a source of a picture whose image has. */
const drawnImage = (element: Element): boolean => {
  const image = element instanceof HTMLSourceElement ? element.parentElement?.querySelector("img") : element;
  return image instanceof HTMLImageElement && image.naturalWidth > 0;
};

/** Gives the elements under `drawn` the signed addresses of those under `fresh`, the nodes of HTML that
 * differs from what `drawn` shows only in its signatures. A link takes the fresh address, and so does an
 * image that hasn't drawn, such as one whose address had run out when it loaded. An image that has drawn
 * keeps its address, and goes on showing without loading again. */
export const resign = (drawn: ParentNode, fresh: ParentNode): void => {
  const shown = drawn.querySelectorAll("*");
  fresh.querySelectorAll("*").forEach((element, i) => {
    const target = shown[i];
    if (target?.tagName !== element.tagName || drawnImage(target)) return;
    for (const { name, value } of element.attributes) {
      if (SIGNATURE.test(value) && target.getAttribute(name) !== value) target.setAttribute(name, value);
    }
  });
};
