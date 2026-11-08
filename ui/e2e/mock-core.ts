import type { Page } from "@playwright/test";
import type { commands } from "../src/lib/data/bindings.ts";

/** What each command answers. A command left out rejects, as it would if the core failed, except for those
 * every page asks: a Repository isn't open yet, a refresh has nothing to say, a mirror can be made and asking
 * to check again has nothing to say, and the panes take their default widths. */
export type CoreAnswers = { [C in keyof typeof commands]?: Awaited<ReturnType<(typeof commands)[C]>> };

type Listener = (message: { event: string; id: number; payload: unknown }) => void;
type MockWindow = { mockEventListeners: Record<string, Listener[]>; mockAnswers: Record<string, unknown> };

/** Stands in for Tauri's IPC: the page's generated bindings get these answers instead of the core's. */
export const mockCore = (page: Page, answers: CoreAnswers) =>
  page.addInitScript((given: Record<string, unknown>) => {
    const panes = { sidebar: null, list: null };
    const answers = {
      lastSync: null,
      syncActivity: null,
      cloning: null,
      mirrorProblem: null,
      checkMirrorsAgain: null,
      refresh: null,
      paneWidths: panes,
      savePaneWidths: { status: "ok", data: panes },
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
        invoke: async (command: string, args: { event: string; handler: number }) => {
          if (command === "plugin:event|listen") {
            (listeners[args.event] ??= []).push(callbacks.get(args.handler) as Listener);
            return callbacks.size;
          }
          // The bindings name commands in camelCase, and the core in snake_case.
          const name = command.replace(/_(.)/g, (_, letter: string) => letter.toUpperCase());
          if (!Object.hasOwn(answers, name)) throw new Error(`the core has no answer for ${command}`);
          // A command that can fail answers as its binding does, and the binding expects the core's own form.
          const answer = (answers as Record<string, unknown>)[name] as {
            status?: string;
            data?: unknown;
            error?: unknown;
          } | null;
          if (answer?.status === "error") throw answer.error;
          return answer?.status === "ok" ? answer.data : answer;
        },
      },
    });
  }, answers);

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
