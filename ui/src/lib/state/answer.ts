// The core's answer to a command that can refuse.

/** The answer of a command that can refuse: its data, or the core's refusal. */
export type Answer<T> = { status: "ok"; data: T } | { status: "error"; error: { message: string } };

/** What any answer of a command that can refuse says: whether it did, and why. */
type Refusable = { status: "ok" } | { status: "error"; error: { message: string } };

/** A call that rejected, read as a refusal. It knows no more of what went wrong than its message. */
type Rejected = { status: "error"; error: { message: string } };

/** The core's refusals come back as answers. Only a broken call rejects, and it reads the same, as does a
 * call that `asking` makes and that throws before it answers. The answer keeps the type the command gave
 * it, so a refusal keeps its `kind`. */
export async function ask<A extends Refusable>(
  asking: Promise<A> | (() => Promise<A>),
): Promise<A | Rejected> {
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
