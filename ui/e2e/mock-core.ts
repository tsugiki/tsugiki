import type { Page } from "@playwright/test";
import type { commands } from "../src/lib/data/bindings.ts";

/** What each command answers. A command left out rejects, as it would if the core failed, except for those
 * every page asks: the build is release 1.2.3, a Repository isn't open yet, GitHub hasn't said the hour's
 * points, a refresh has nothing to say, a mirror can be made, asking to check again has nothing to say, no
 * mirror waits for an answer or has a state, no newer version is out, the panes take their default layout,
 * the palette has no recent commands and puts each run first, the themes set no token, so the system's
 * colours stand in, the window turns without saying a side, so the page goes on with its media query, and
 * the core takes what the user did with an item or the window. */
export type CoreAnswers = { [C in keyof typeof commands]?: Awaited<ReturnType<(typeof commands)[C]>> };

type Listener = (message: { event: string; id: number; payload: unknown }) => void;
type MockWindow = { mockEventListeners: Record<string, Listener[]>; mockAnswers: Record<string, unknown> };

/** The answers a test gives, and the commands it has reject. */
type Given = { given: Record<string, unknown>; refused: string[] };

/** Stands in for Tauri's IPC: the page's generated bindings get these answers instead of the core's. The
 * commands in `unanswered` reject, even one this mock answers when a test doesn't. */
export const mockCore = (page: Page, answers: CoreAnswers, unanswered: (keyof CoreAnswers)[] = []) => {
  const test: Given = { given: answers, refused: unanswered };
  return page.addInitScript(({ given, refused }: Given) => {
    const panes = {
      sidebar: null,
      list: null,
      peek: null,
      split_peek: null,
      rail: null,
      sidebar_hidden: false,
      sidebar_icons: false,
      list_hidden: false,
      detail_only_hid: null,
      order: "attention",
    };
    const answers: Record<string, unknown> = {
      about: { name: "Tsugiki", version: "1.2.3", build: "release", source_ref: "v1.2.3" },
      lastSync: null,
      syncActivity: null,
      hourPoints: null,
      cloning: null,
      mirrorProblem: null,
      checkMirrorsAgain: null,
      mirrorQuestion: null,
      mirrorStates: [],
      mirrorsHeld: false,
      newVersion: null,
      refresh: null,
      hideItem: { status: "ok", data: null },
      showItem: { status: "ok", data: null },
      closeItem: { status: "ok", data: null },
      windowLoaded: null,
      windowHidden: null,
      windowShown: null,
      paneLayout: panes,
      themes: {
        status: "ok",
        data: {
          theme: { kind: "system" },
          light: { chosen: "Light", all: [{ name: "Light", kind: "light", tokens: [] }], failed: [] },
          dark: { chosen: "Dark", all: [{ name: "Dark", kind: "dark", tokens: [] }], failed: [] },
          problems: [],
        },
      },
      savePaneLayout: { status: "ok", data: panes },
      // What `machine.toml` holds of the palette's recent commands: a run changes it, as the core does.
      recentCommands: [] as string[],
      turnWindow: null,
      ...given,
    };
    const listeners: MockWindow["mockEventListeners"] = {};
    const callbacks = new Map<number, Listener>();
    Object.assign(window, {
      mockEventListeners: listeners,
      mockAnswers: answers,
      __TAURI_INTERNALS__: {
        transformCallback: (callback: Listener) => {
          const id = callbacks.size + 1;
          callbacks.set(id, callback);
          return id;
        },
        invoke: async (command: string, args: { event: string; handler: number; command: string }) => {
          if (command === "plugin:event|listen") {
            (listeners[args.event] ??= []).push(callbacks.get(args.handler) as Listener);
            return callbacks.size;
          }
          // The bindings name commands in camelCase, and the core in snake_case.
          const name = command.replace(/_(.)/g, (_, letter: string) => letter.toUpperCase());
          if (refused.includes(name)) throw new Error(`the core has no answer for ${command}`);
          // Unless the test answers it, a run comes first in the list `recentCommands` answers with, and the
          // run answers with that list too.
          if (command === "remember_command" && !Object.hasOwn(answers, "rememberCommand")) {
            const recent = answers.recentCommands as string[];
            answers.recentCommands = [args.command, ...recent.filter((id) => id !== args.command)];
            return answers.recentCommands;
          }
          if (!Object.hasOwn(answers, name)) throw new Error(`the core has no answer for ${command}`);
          // A command that can fail answers as its binding does, and the binding expects the core's own form.
          const answer = answers[name] as {
            status?: string;
            data?: unknown;
            error?: unknown;
          } | null;
          if (answer?.status === "error") throw answer.error;
          return answer?.status === "ok" ? answer.data : answer;
        },
      },
    });
  }, test);
};

/** Changes what a command answers from now on, as the core's facts would. */
export const changeAnswer = <C extends keyof CoreAnswers>(page: Page, command: C, answer: CoreAnswers[C]) =>
  page.evaluate(
    ([command, answer]) => {
      (window as unknown as MockWindow).mockAnswers[command as string] = answer;
    },
    [command, answer] as const,
  );

/** Sends an event to the page, as the core would. */
export const emitFromCore = (page: Page, event: string, payload: unknown) =>
  page.evaluate(
    ([event, payload]) => {
      const name = event as string;
      const { mockEventListeners } = window as unknown as MockWindow;
      for (const listener of mockEventListeners[name] ?? []) listener({ event: name, id: 1, payload });
    },
    [event, payload],
  );
