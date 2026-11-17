// The core's answer to a command that can refuse.

/** A refusal as any command's answer carries it, and a call that rejected reads as one. Such a call knows no
 * more of what went wrong than its message. */
type Refusal = { status: "error"; error: { message: string } };

/** The answer of a command that can refuse: its data, or the core's refusal. */
export type Answer<T> = { status: "ok"; data: T } | Refusal;

/** What any answer of a command that can refuse says: whether it did, and why. */
type Refusable = { status: "ok" } | Refusal;

/** The core's refusals come back as answers. Only a broken call rejects, and it reads the same, as does a
 * call that `asking` makes and that throws before it answers. The answer keeps the type the command gave
 * it, so a refusal keeps its `kind`. */
export async function ask<A extends Refusable>(
  asking: Promise<A> | (() => Promise<A>),
): Promise<A | Refusal> {
  try {
    return await (typeof asking === "function" ? asking() : asking);
  } catch (e) {
    return { status: "error", error: { message: String(e) } };
  }
}

/** The problem a command ran into, or nothing if it did what it was asked. */
export async function problemIn(
  asking: Promise<Refusable> | (() => Promise<Refusable>),
): Promise<string | undefined> {
  const answer = await ask(asking);
  return answer.status === "ok" ? undefined : answer.error.message;
}
