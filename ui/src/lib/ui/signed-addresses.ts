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

/** A signed attribute of a drawn element, and the fresh value it takes. */
type Change = [attribute: Attr, value: string];

/** Adds to `changes` each signed attribute of `element`, drawn, that takes its value in `fresh`, unless
 * `element` keeps its address. Gives `false` where `element` has lost an attribute `fresh` signs. */
const signedChanges = (element: Element, fresh: Element, changes: Change[]): boolean => {
  if (keepsAddress(element)) return true;
  for (const { name, value } of fresh.attributes) {
    if (!SIGNATURE.test(value) || element.getAttribute(name) === value) continue;
    const attribute = element.getAttributeNode(name);
    if (!attribute) return false;
    changes.push([attribute, value]);
  }
  return true;
};

/** Lines up the nodes under `drawn` with those under `fresh`, child by child, and adds to `changes` the
 * signed attributes to give fresh values. Gives `false` where the two differ in shape, a tag or text. */
const lineUp = (drawn: Node, fresh: Node, changes: Change[]): boolean => {
  const shown = drawn.childNodes;
  const wanted = Array.from(fresh.childNodes);
  return (
    shown.length === wanted.length &&
    wanted.every((node, i) => {
      const target = shown[i] as Node;
      if (target.nodeName !== node.nodeName) return false;
      if (!(node instanceof Element)) return target.nodeValue === node.nodeValue;
      return signedChanges(target as Element, node, changes) && lineUp(target, node, changes);
    })
  );
};

/** Gives the elements under `drawn` the signed addresses of those under `fresh`, the nodes of HTML that
 * differs from what `drawn` drew only in its signatures. A link takes the fresh address, and so does an
 * image that failed. An image that has drawn keeps its address, and goes on showing without loading again,
 * and so does one still loading. Gives `false`, and changes nothing, unless the two have the same shape,
 * tags and text, since a signature in the text, or an element added since, needs drawing afresh. Setting an
 * attribute's node can't fail, so the changes land all together. */
export const resign = (drawn: Node, fresh: Node): boolean => {
  const changes: Change[] = [];
  if (!lineUp(drawn, fresh, changes)) return false;
  for (const [attribute, value] of changes) attribute.value = value;
  return true;
};
