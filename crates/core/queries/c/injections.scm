; From Helix: https://github.com/helix-editor/helix/blob/ba40e547426b0f9896c8bdc699a4ab11f2b37dbc/runtime/queries/c/injections.scm
; Unchanged. See docs/highlighting.md for how to update it.
;
; This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of the MPL
; was not distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.

((comment) @injection.content
 (#set! injection.language "comment"))

((preproc_arg) @injection.content
 (#set! injection.language "c")
 (#set! injection.include-children))
