// The core's answer to a command that can refuse.

/** The answer of a command that can refuse: its data, or the core's refusal. */
export type Answer<T> = { status: "ok"; data: T } | { status: "error"; error: { message: string } };

/** The core's refusals come back as answers. Only a broken call rejects, and it reads the same. */
export async function ask<T>(asking: Promise<Answer<T>>): Promise<Answer<T>> {
  try {
    return await asking;
  } catch (e) {
    return { status: "error", error: { message: String(e) } };
  }
}
