// The pages of the Files tab when it shows one file at a time (#605): the PR's NEWS entries first, as the
// tab shows them on top of all its files, then each file in the order of the all-files view, then the
// threads on files the PR no longer changes, which sit after the last file there.
import type { DiffFiles } from "$lib/data/bindings";

/** Whether the Files tab shows all the PR's files, one under the other, or one file at a time. */
export type FilesMode = DiffFiles;

/** A page of the tab: the NEWS entries, the file at `file` among the diff's files, or the threads on files
 * no longer in the diff. */
export type Page = { kind: "news" } | { kind: "file"; file: number } | { kind: "gone" };

/** Whether `one` and `other` are the same page. */
export function samePage(one: Page | undefined, other: Page | undefined): boolean {
  if (one?.kind === "file" && other?.kind === "file") return one.file === other.file;
  return one !== undefined && one.kind === other?.kind;
}

/** The pages of one diff, in order. */
export class FilePages {
  readonly #paths: readonly string[];
  readonly #news: boolean;
  readonly #gone: boolean;

  /** The pages of a diff whose files have `paths`, in its order, with a page of NEWS entries when the PR
   * adds any, and a last page when some threads are on files no longer in the diff. */
  constructor({ paths, news, gone }: { paths: readonly string[]; news: boolean; gone: boolean }) {
    this.#paths = paths;
    this.#news = news;
    this.#gone = gone;
  }

  /** The page the tab starts on, if the diff has any. */
  get first(): Page | undefined {
    return this.#at(0);
  }

  /** Whether the diff has `page`. */
  has(page: Page): boolean {
    return this.#index(page) !== -1;
  }

  /** The page after `page`, or by `step` -1 the one before, or nothing past either end. */
  step(page: Page, step: 1 | -1): Page | undefined {
    const at = this.#index(page);
    return at === -1 ? undefined : this.#at(at + step);
  }

  /** The page of the file at `path`, if the diff changes it. */
  ofPath(path: string): Page | undefined {
    const file = this.#paths.indexOf(path);
    return file === -1 ? undefined : { kind: "file", file };
  }

  /** The place of a file's page among the files, counted from 1, as the bar says "12 of 48", with its
   * path. Nothing for a page that shows no file. */
  counted(page: Page): { at: number; count: number; path: string } | undefined {
    const path = page.kind === "file" ? this.#paths[page.file] : undefined;
    return page.kind === "file" && path !== undefined
      ? { at: page.file + 1, count: this.#paths.length, path }
      : undefined;
  }

  /** The page of this diff that shows what `page` of `before`, an older diff of the PR, showed: the same
   * file by its path, the NEWS entries, or the threads on files no longer in the diff, and otherwise the
   * first page. */
  carried(page: Page, before: FilePages): Page | undefined {
    const path = page.kind === "file" ? before.#paths[page.file] : undefined;
    const same = path === undefined ? page : this.ofPath(path);
    return same !== undefined && this.has(same) ? same : this.first;
  }

  /** Where the first file's page comes among the pages: after the NEWS entries, if there are any. */
  get #firstFile(): number {
    return this.#news ? 1 : 0;
  }

  /** Where `page` comes among the pages, or -1 if the diff doesn't have it. A diff can have thousands of
   * files, so this counts rather than lists them. */
  #index(page: Page): number {
    const files = this.#paths.length;
    switch (page.kind) {
      case "news":
        return this.#news ? 0 : -1;
      case "file":
        return page.file >= 0 && page.file < files ? this.#firstFile + page.file : -1;
      case "gone":
        return this.#gone ? this.#firstFile + files : -1;
    }
  }

  /** The page at `index` among the pages, if there is one. */
  #at(index: number): Page | undefined {
    const file = index - this.#firstFile;
    if (index < 0) return undefined;
    if (file < 0) return { kind: "news" };
    if (file < this.#paths.length) return { kind: "file", file };
    return this.#gone && file === this.#paths.length ? { kind: "gone" } : undefined;
  }
}
