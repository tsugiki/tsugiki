import type { Page } from "@playwright/test";
import type { commands } from "../src/lib/data/bindings.ts";

/** What each command answers. A command left out rejects, as it would if the core failed. */
export type CoreAnswers = { [C in keyof typeof commands]?: Awaited<ReturnType<(typeof commands)[C]>> };

/** Stands in for Tauri's IPC: the page's generated bindings get these answers instead of the core's. */
export const mockCore = (page: Page, answers: CoreAnswers) =>
  page.addInitScript((answers: Record<string, unknown>) => {
    Object.assign(window, {
      __TAURI_INTERNALS__: {
        invoke: async (command: string) => {
          if (Object.hasOwn(answers, command)) return answers[command];
          throw new Error(`the core has no answer for ${command}`);
        },
      },
    });
  }, answers);
