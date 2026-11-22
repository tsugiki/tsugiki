# tsugiki-token lets through reads and the writes it lists

[ADR 0019](0019-the-token-and-the-only-code-that-sends-it-have-a-crate-of-their-own.md) keeps the token on
one path, but not what travels with it. `post_graphql` sends any body, so the core can send any mutation
GitHub offers, merge and close among them. `run_gh` runs gh with any arguments and gh's own token, outside
`Https`. To learn what Tsugiki can change on GitHub, you would have to read the core. With gh's token, the
default, that covers each repository you can push to.

So the crate decides what may change GitHub, as well as where the token goes:

- gh runs with one set of arguments, `auth token --hostname github.com`.
- A read goes through `post_graphql`. The crate parses the body into `query` and `variables`, and refuses
  one with another key, a key twice, or a `query` that contains `mutation` anywhere. GraphQL writes only
  through an operation that starts with that keyword, and has no other spelling for it. The crate sends its
  own serialization of what it checked.
- A write goes through `post_write` and names a variant of `Write`, a closed enum with one variant per
  mutation and typed inputs. The crate holds each mutation's text. The core adds the selection set of the
  answer and its fragments, so a write's answer parses as a read's does. The crate refuses a selection with
  `mutation`, `"` or `#` in it, a payload that isn't one selection set, closed by its last `}`, or fragments
  whose braces don't pair up, so the core can't add a second mutation beside the first. The node IDs a
  write takes are the crate's `NodeId`, which the core parses GitHub's answers into.
- REST stays GET only.

The README lists each write in a table. A new write changes the crate, and a reviewer looks there.

## Considered Options

- **The query builders in the crate as well.** You would see each read, but a read can't change GitHub, and
  its answer can only go back to `api.github.com`. Each builder lists the fields a serde struct in the core
  reads, so a new field would touch two crates, and the builders use the core's types. ADR 0019 turned this
  down for the same reasons.
- **A list of the reads' texts.** Several queries add an alias per number or per Filter at run time, so the
  list would have to cover each count.
- **A GraphQL parser in the crate.** It answers the question the word check answers, with a dependency an
  auditor has to trust.
- **Checking the JSON text without parsing it.** Variables written as `null, "query": "mutation …"` would add
  a second `query` key, and a hand-written scan of JSON is harder to audit than serde's derive.
- **A narrower token alone** ([ADR 0004](0004-auth-via-gh-cli-token.md)). GitHub enforces it, and it stays
  the strongest guard. gh's token stays the default, and this rule protects the users who keep it.

## Consequences

- The crate's dependencies gain `serde` and `serde_json`, which the core already builds. ADR 0019's rule for
  what belongs in the crate gains a clause: the JSON the crate checks before it sends. Reading GitHub's
  answers stays in the core.
- Crate privacy guards against mistakes, and lets an auditor read one crate. It doesn't stop hostile code in
  the same process: a compromised dependency of the core could open its own socket or read memory. A
  narrower token guards against that.
- `Https`'s methods take `&Token` as before, so the core's test doubles keep their shape.
