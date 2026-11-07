; Tsugiki's own. tree-sitter-svelte-ng's query is written for Neovim, which reads it after HTML's, so it
; can't say which language a <script> or a <style> is in on its own. Of the patterns that inject into one
; node, the last that names a language Tsugiki highlights wins.

; The code inside braces: `{count + 1}`, `{#if ready}`.
((svelte_raw_text) @injection.content
  (#set! injection.language "javascript"))

((script_element
  (raw_text) @injection.content)
  (#set! injection.language "javascript"))

((style_element
  (raw_text) @injection.content)
  (#set! injection.language "css"))

; `<script lang="ts">`, or a `lang` Tsugiki doesn't highlight, which leaves the language above.
((_
  (start_tag
    (attribute
      (attribute_name) @_attr
      (quoted_attribute_value
        (attribute_value) @injection.language)))
  (raw_text) @injection.content)
  (#eq? @_attr "lang"))
