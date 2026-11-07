; From Helix: https://github.com/helix-editor/helix/blob/ba40e547426b0f9896c8bdc699a4ab11f2b37dbc/runtime/queries/rst/highlights.scm
; Unchanged. See docs/highlighting.md for how to update it.
;
; This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of the MPL
; was not distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.

(comment) @comment

[
  (title)
] @markup.heading.1

[
  "adornment"
] @markup.heading.marker

[
  (target)
  (reference)
] @markup.link.url

[
  "bullet"
] @markup.list.unnumbered

(strong) @markup.bold
(emphasis) @markup.italic
(literal) @markup.raw.inline

(list_item
  (term) @markup.bold
  (classifier)? @markup.italic)

(directive
  [".." (type) "::"] @function
)

(field
  [":" (field_name) ":"] @variable.other.member
)

(interpreted_text) @markup.raw.inline

(interpreted_text (role)) @keyword
