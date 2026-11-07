; Tsugiki's own. The content of a code-block, code or sourcecode directive, in the language its argument
; names: `.. code-block:: python`.

((directive
  name: (type) @_type
  body: (body
    (arguments) @injection.language
    (content) @injection.content))
  (#any-of? @_type "code" "code-block" "sourcecode"))
