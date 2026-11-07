; Tsugiki's own. tree-sitter-md's query parses each HTML tag in a paragraph on its own, so a closing tag
; such as `</kbd>`, which isn't HTML alone, stays plain. Here a paragraph's tags parse as one.

((html_tag) @injection.content
  (#set! injection.language "html")
  (#set! injection.combined))
