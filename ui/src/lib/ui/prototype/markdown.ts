// PROTOTYPE, throwaway (#367): a crude stand-in for GitHub's Markdown, enough to judge where a preview
// goes. The real preview would come from GitHub's renderer or the core's, which is an open question.

const escape = (text: string) =>
  text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");

const inline = (text: string) =>
  escape(text)
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/(^|\W)\*([^*]+)\*/g, "$1<em>$2</em>")
    .replace(/(^|\s)@([\w-]+)/g, '$1<span class="mention">@$2</span>')
    .replace(/(^|\s)#(\d+)/g, '$1<span class="ref">#$2</span>');

/** Paragraphs, fenced code, a `suggestion` block shown as the change it suggests, and inline code. */
export function render(source: string, replaced: string[] = []): string {
  const out: string[] = [];
  const lines = source.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i] ?? "";
    const fence = /^```(\w*)/.exec(line);
    if (fence) {
      const body: string[] = [];
      for (i++; i < lines.length && !(lines[i] ?? "").startsWith("```"); i++) body.push(lines[i] ?? "");
      if (fence[1] === "suggestion") {
        const rows = [
          ...replaced.map((one) => `<div class="del">- ${escape(one)}</div>`),
          ...body.map((one) => `<div class="add">+ ${escape(one)}</div>`),
        ];
        out.push(`<div class="suggestion"><div class="shead">Suggested change</div>${rows.join("")}</div>`);
      } else out.push(`<pre><code>${escape(body.join("\n"))}</code></pre>`);
      continue;
    }
    if (/^>\s?/.test(line)) {
      out.push(`<blockquote>${inline(line.replace(/^>\s?/, ""))}</blockquote>`);
      continue;
    }
    if (/^[-*] /.test(line)) {
      out.push(`<ul><li>${inline(line.slice(2))}</li></ul>`);
      continue;
    }
    if (line.trim() === "") continue;
    const para = [line];
    while (i + 1 < lines.length && (lines[i + 1] ?? "").trim() !== "" && !/^(```|>|[-*] )/.test(lines[i + 1] ?? ""))
      para.push(lines[++i] ?? "");
    out.push(`<p>${inline(para.join(" "))}</p>`);
  }
  return out.join("") || '<p class="nothing">Nothing to preview</p>';
}
