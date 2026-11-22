// The Filters of each Repository as its page in Settings edits them (docs/devel/settings.md, #535): renamed,
// searching something else, moved, deleted or added, until Apply writes them to `settings.toml`.
import type {
  EditedFilter_Deserialize,
  Filter_Serialize,
  RepositoryFilters_Deserialize,
} from "$lib/data/bindings";

/** A Filter on a Repository's page: its name and search as the user typed them, and the search it had when
 * the dialog read the files, which keeps its table in the file through a new name, a new search or a move, so
 * a key the page doesn't show, such as `on_workbench`, stays. Null for a Filter Add filter added. `id` keeps
 * its row on the page through a move. */
export interface FilterRow {
  readonly id: number;
  name: string;
  query: string;
  readonly was: string | null;
}

/** A Repository as the files hold it, with its Filters. */
interface Held {
  name: string;
  filters: Filter_Serialize[];
}

/** Whether `row` has a name or a search that is blank, which the file can't hold. */
export const isBlank = (row: FilterRow) => row.name.trim() === "" || row.query.trim() === "";

/** A list of Filters as one string, in its order, to tell whether the rows still say what the files did. */
const saying = (filters: { was: string | null; name: string; query: string }[]) =>
  JSON.stringify(filters.map(({ was, name, query }) => [was, name.trim(), query.trim()]));

/** The Filters of one Repository's page. */
export class FilterPage {
  rows = $state<FilterRow[]>([]);
  /** The Filters the files held, which Apply sends for the core to check the file holds them still. */
  readonly read: Filter_Serialize[];
  /** The Filters the files held, as `saying` puts them. */
  readonly #held: string;
  #ids = 0;

  constructor(filters: Filter_Serialize[]) {
    this.read = structuredClone(filters);
    this.rows = filters.map(({ name, query }) => this.#row(name, query, query));
    this.#held = saying(filters.map(({ name, query }) => ({ was: query, name, query })));
  }

  /** Adds a Filter with a blank name and search at the end. */
  add() {
    this.rows.push(this.#row("", "", null));
  }

  /** Moves the Filter at `at` one place up or down. The page offers no move past either end. */
  move(at: number, by: -1 | 1) {
    this.rows.splice(at + by, 0, ...this.rows.splice(at, 1));
  }

  /** Takes the Filter at `at` out. */
  delete(at: number) {
    this.rows.splice(at, 1);
  }

  /** Whether a row has a blank name or search. */
  get blank(): boolean {
    return this.rows.some(isBlank);
  }

  /** Whether the rows say other than the files did. */
  get changed(): boolean {
    return saying(this.rows) !== this.#held;
  }

  /** The Filters as Apply saves them. */
  get edited(): EditedFilter_Deserialize[] {
    return this.rows.map(({ name, query, was }) => ({ name: name.trim(), query: query.trim(), was }));
  }

  #row(name: string, query: string, was: string | null): FilterRow {
    this.#ids += 1;
    return { id: this.#ids, name, query, was };
  }
}

export class FilterEdits {
  /** The page of each Repository, by name, once the dialog has read the files. */
  #pages = $state.raw<Record<string, FilterPage>>({});

  /** Makes each Repository's page afresh from `repositories`, as the files say now. */
  take(repositories: Held[]) {
    this.#pages = {};
    for (const repository of repositories) this.add(repository);
  }

  /** Makes the page of `repository`, unless it has one: of a Repository written twice the first counts, as
   * the core edits the first. */
  add(repository: Held) {
    if (repository.name in this.#pages) return;
    this.#pages = { ...this.#pages, [repository.name]: new FilterPage(repository.filters) };
  }

  /** The page of `repository`, or nothing before the dialog has read the files. */
  page(repository: string): FilterPage | undefined {
    return this.#pages[repository];
  }

  /** The first Repository with a Filter whose name or search is blank, which keeps Apply off, if any. */
  get blankIn(): string | undefined {
    return Object.entries(this.#pages).find(([, page]) => page.blank)?.[0];
  }

  /** The Filters of each Repository whose rows say other than the files did, which Apply saves, with those
   * the files held. */
  get changes(): RepositoryFilters_Deserialize[] {
    return Object.entries(this.#pages)
      .filter(([, page]) => page.changed)
      .map(([repository, page]) => ({ repository, read: page.read, filters: page.edited }));
  }
}
