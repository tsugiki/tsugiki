import { createEvent, fireEvent, render, screen } from "@testing-library/svelte";
import { expect, it, vi } from "vitest";
import RenderedHtml from "./RenderedHtml.svelte";

const shown = (html: string) => {
  const onlink = vi.fn();
  render(RenderedHtml, { html, onlink });
  return onlink;
};

/** Clicks `element`, and says whether the browser may still act on the click. */
const clickFollows = (element: Element, kind: "click" | "auxclick" = "click", button = 0) => {
  const click = new MouseEvent(kind, { bubbles: true, cancelable: true, button });
  void fireEvent(element, click);
  return !click.defaultPrevented;
};

it("shows GitHub's rendering", () => {
  shown('<p>It <em>crashes</em></p><pre><code class="language-python">x = 1</code></pre>');

  expect(screen.getByText("crashes").tagName).toBe("EM");
  expect(screen.getByText("x = 1").tagName).toBe("CODE");
});

it("sends a clicked link to the browser and keeps the window where it is", () => {
  const onlink = shown('<p><a href="https://github.com/python/cpython/issues/1">gh-1</a></p>');

  expect(clickFollows(screen.getByText("gh-1"))).toBe(false);

  expect(onlink).toHaveBeenCalledExactlyOnceWith("https://github.com/python/cpython/issues/1");
});

it("does the same for a middle click, and for a click on what a link holds", () => {
  const onlink = shown('<a href="https://github.com/"><em>inside</em></a>');

  expect(clickFollows(screen.getByText("inside"), "auxclick", 1)).toBe(false);

  expect(onlink).toHaveBeenCalledExactlyOnceWith("https://github.com/");
});

it("leaves a right click and the side buttons alone", () => {
  const onlink = shown('<a href="https://github.com/">Home</a>');

  for (const button of [2, 3, 4]) {
    expect(clickFollows(screen.getByText("Home"), "auxclick", button)).toBe(true);
  }

  expect(onlink).not.toHaveBeenCalled();
});

it("keeps a link within the text and one with nowhere to go from leaving", () => {
  const onlink = shown('<a href="#a-heading">Near</a><a>Nowhere</a>');

  expect(clickFollows(screen.getByText("Near"))).toBe(false);
  expect(clickFollows(screen.getByText("Nowhere"))).toBe(false);

  expect(onlink).not.toHaveBeenCalled();
});

it("says where a link goes in its tooltip, from its address rather than its words", () => {
  shown('<p><a href="https://discuss.python.org/t/original-3-13-patch/123">original 3.13 patch</a></p>');

  expect(screen.getByText("original 3.13 patch").getAttribute("title")).toBe(
    "Open in the browser: https://discuss.python.org/t/original-3-13-patch/123",
  );
});

it("shows the whole address, even a long GitHub path", () => {
  const address = "https://github.com/python/cpython/blob/main/Doc/whatsnew/3.13.rst#notes";
  shown(`<a href="${address}">the notes</a>`);

  expect(screen.getByText("the notes").getAttribute("title")).toBe(`Open in the browser: ${address}`);
});

it("gives no tooltip to a link within the text or one with nowhere to go", () => {
  shown('<a href="#a-heading">Near</a><a>Nowhere</a>');

  expect(screen.getByText("Near").hasAttribute("title")).toBe(false);
  expect(screen.getByText("Nowhere").hasAttribute("title")).toBe(false);
});

it("leaves a link with nowhere to go the tooltip the HTML itself gave it", () => {
  shown('<a title="GitHub wrote this">Nowhere</a>');

  expect(screen.getByText("Nowhere").getAttribute("title")).toBe("GitHub wrote this");
});

it("leaves other clicks to the browser, so a folded section still unfolds", () => {
  const onlink = shown("<details><summary>More</summary>Inside</details>");

  expect(clickFollows(screen.getByText("More"))).toBe(true);

  expect(onlink).not.toHaveBeenCalled();
});

it("keeps a form from submitting the window away", () => {
  shown('<form action="https://github.com/" data-testid="form"></form>');
  const form = screen.getByTestId("form");
  const submit = createEvent.submit(form);

  void fireEvent(form, submit);

  expect(submit.defaultPrevented).toBe(true);
});

it("drops a refresh and a base address, and shows the rest", () => {
  const { container } = render(RenderedHtml, {
    html: '<meta http-equiv="refresh" content="0; url=/"><base href="https://github.com/"><p>Read this</p>',
    onlink: vi.fn(),
  });

  expect(screen.getByText("Read this").tagName).toBe("P");
  expect(container.querySelectorAll("meta, base")).toHaveLength(0);
});

it("shows new HTML in place of the old", async () => {
  const { rerender } = render(RenderedHtml, { html: "<p>Before</p>", onlink: vi.fn() });

  await rerender({ html: "<p>After</p>" });

  expect(screen.getByText("After").tagName).toBe("P");
  expect(screen.queryByText("Before")).toBeNull();
});

/** An upload's address as GitHub's rendered HTML gives it, with the query it signs afresh on each fetch. */
const upload = (signature: string) =>
  `https://private-user-images.githubusercontent.com/1/2-a1b2.png?jwt=${signature}`;

/** A folded section holding an upload, as GitHub renders a screenshot, with its link. */
const screenshot = (signature: string, words = "Steps") =>
  `<details><summary>${words}</summary><a href="${upload(signature)}">` +
  `<img alt="upload" src="${upload(signature)}"></a>` +
  `<picture><source media="(prefers-color-scheme: dark)" srcset="${upload(signature)}">` +
  `<img alt="variant" src="${upload(signature)}"></picture></details>`;

/** Says `image` has finished loading, `width` pixels wide, or failed with 0. jsdom loads no image, and says
 * each is still loading. */
const loaded = (image: HTMLElement, width: number) =>
  Object.defineProperties(image, { complete: { value: true }, naturalWidth: { value: width } });

it("keeps what it drew when a refresh signs its uploads' addresses afresh", async () => {
  const { rerender } = render(RenderedHtml, { html: screenshot("one"), onlink: vi.fn() });
  const image = screen.getByAltText("upload");
  loaded(image, 20);
  loaded(screen.getByAltText("variant"), 20);
  const section = image.closest("details") as HTMLDetailsElement;
  section.open = true;

  await rerender({ html: screenshot("two") });

  expect(screen.getByAltText("upload")).toBe(image);
  expect(section.open).toBe(true);
  expect(image.getAttribute("src")).toBe(upload("one"));
  expect(section.querySelector("source")?.getAttribute("srcset")).toBe(upload("one"));
  expect(image.closest("a")?.getAttribute("href")).toBe(upload("two"));
});

it("gives a kept link the fresh address in its tooltip", async () => {
  const { rerender } = render(RenderedHtml, { html: screenshot("one"), onlink: vi.fn() });
  loaded(screen.getByAltText("upload"), 20);
  loaded(screen.getByAltText("variant"), 20);

  await rerender({ html: screenshot("two") });

  expect(screen.getByAltText("upload").closest("a")?.getAttribute("title")).toBe(
    `Open in the browser: ${upload("two")}`,
  );
});

it("gives an image that failed its fresh address", async () => {
  const { rerender } = render(RenderedHtml, { html: screenshot("run-out"), onlink: vi.fn() });
  const image = screen.getByAltText("upload");
  loaded(image, 0);
  loaded(screen.getByAltText("variant"), 0);

  await rerender({ html: screenshot("fresh") });

  expect(screen.getByAltText("upload")).toBe(image);
  expect(image.getAttribute("src")).toBe(upload("fresh"));
  expect(image.closest("details")?.querySelector("source")?.getAttribute("srcset")).toBe(upload("fresh"));
  expect(screen.getByAltText("variant").getAttribute("src")).toBe(upload("fresh"));
});

it("lets an image still loading go on with the address it has", async () => {
  const { rerender } = render(RenderedHtml, { html: screenshot("one"), onlink: vi.fn() });
  const image = screen.getByAltText("upload");

  await rerender({ html: screenshot("two") });

  expect(screen.getByAltText("upload")).toBe(image);
  expect(image.getAttribute("src")).toBe(upload("one"));
  expect(image.closest("a")?.getAttribute("href")).toBe(upload("two"));
});

it("draws afresh HTML whose text changed in an upload's address", async () => {
  const text = (signature: string) => `<p><code>${upload(signature)}</code></p>`;
  const { rerender } = render(RenderedHtml, { html: text("one"), onlink: vi.fn() });

  await rerender({ html: text("two") });

  expect(screen.getByText(upload("two")).tagName).toBe("CODE");
});

it("draws afresh when an element it drew became another", async () => {
  const { container, rerender } = render(RenderedHtml, { html: screenshot("one"), onlink: vi.fn() });
  screen.getByAltText("upload").replaceWith(document.createElement("span"));

  await rerender({ html: screenshot("two") });

  expect(screen.getByAltText("upload").getAttribute("src")).toBe(upload("two"));
  expect(container.querySelector("span")).toBeNull();
});

it("draws afresh when an element it drew moved, though the nodes keep their order", async () => {
  const { container, rerender } = render(RenderedHtml, { html: screenshot("one"), onlink: vi.fn() });
  const section = container.querySelector("details") as HTMLDetailsElement;
  section.after(section.querySelector("picture") as HTMLPictureElement);

  await rerender({ html: screenshot("two") });

  expect(container.querySelector("details picture")).not.toBeNull();
  expect(screen.getByAltText("variant").getAttribute("src")).toBe(upload("two"));
});

it("draws afresh when an element it drew lost a signed attribute", async () => {
  const { rerender } = render(RenderedHtml, { html: screenshot("one"), onlink: vi.fn() });
  const link = screen.getByAltText("upload").closest("a") as HTMLAnchorElement;
  link.removeAttribute("href");

  await rerender({ html: screenshot("two") });

  const now = screen.getByAltText("upload").closest("a");
  expect(now).not.toBe(link);
  expect(now?.getAttribute("href")).toBe(upload("two"));
});

it("draws afresh when an element came under what it drew", async () => {
  const { container, rerender } = render(RenderedHtml, { html: screenshot("one"), onlink: vi.fn() });
  const image = screen.getByAltText("upload");
  loaded(image, 0);
  image.before(document.createElement("img"));

  await rerender({ html: screenshot("two") });

  expect(screen.getByAltText("upload")).not.toBe(image);
  expect(screen.getByAltText("upload").getAttribute("src")).toBe(upload("two"));
  expect(container.querySelectorAll("img")).toHaveLength(2);
});

it("draws afresh HTML that differs past its signatures", async () => {
  const { rerender } = render(RenderedHtml, { html: screenshot("one"), onlink: vi.fn() });
  const image = screen.getByAltText("upload");

  await rerender({ html: screenshot("two", "Edited steps") });

  expect(screen.getByText("Edited steps").tagName).toBe("SUMMARY");
  expect(screen.getByAltText("upload")).not.toBe(image);
});

it("takes a query on another host as a change", async () => {
  const image = (signature: string) =>
    `<img alt="badge" src="https://camo.githubusercontent.com/a?jwt=${signature}">`;
  const { rerender } = render(RenderedHtml, { html: image("one"), onlink: vi.fn() });
  const before = screen.getByAltText("badge");

  await rerender({ html: image("two") });

  expect(screen.getByAltText("badge")).not.toBe(before);
  expect(screen.getByAltText("badge").getAttribute("src")).toBe(
    "https://camo.githubusercontent.com/a?jwt=two",
  );
});
