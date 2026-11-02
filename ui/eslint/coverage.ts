// An exclusion from TypeScript's coverage gate says why, on the line above, where review can see it
// (ADR 0009). The hints here are the ones ast-v8-to-istanbul honours, in script and in markup alike.
import type { AST, ESLint, Rule } from "eslint";

const EXCLUSION = /(?:istanbul|[cv]8|node:coverage)\s+ignore\s+(?:if|else|next|file|start)(?=\W|$)/;
const REASON = /^\s*(?:\/\/|<!--)\s*coverage:\s*[^\s-]/;

const reason: Rule.RuleModule = {
  meta: {
    type: "problem",
    docs: { description: "Require a reason on the line above every coverage exclusion" },
    schema: [],
    messages: { missing: "a coverage exclusion needs a `// coverage: <reason>` comment on the line above" },
  },
  create(context) {
    const { sourceCode } = context;
    const check = (text: string, loc: AST.SourceLocation) => {
      if (EXCLUSION.test(text) && !REASON.test(sourceCode.lines[loc.start.line - 2] ?? "")) {
        context.report({ loc, messageId: "missing" });
      }
    };
    return {
      Program() {
        for (const comment of sourceCode.getAllComments()) check(comment.value, comment.loc!);
      },
      // Svelte's template comments aren't among the script's comments.
      SvelteHTMLComment(node: Rule.Node) {
        const { value, loc } = node as unknown as { value: string; loc: AST.SourceLocation };
        check(value, loc);
      },
    };
  },
};

const coverage = { meta: { name: "tsugiki-coverage" }, rules: { reason } } satisfies ESLint.Plugin;
export default coverage;
