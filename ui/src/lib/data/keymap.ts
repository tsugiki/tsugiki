// Generated from tsugiki-core's key table by `npm run bindings`. Do not edit.

/** Each action the user can bind, with its title, default key and where it works. */
export const KEYMAP = [
  {
    "action": "command_palette",
    "title": "Command Palette",
    "key": "ctrl+shift+p",
    "context": "anywhere"
  },
  {
    "action": "quick_open",
    "title": "Quick Open",
    "key": "ctrl+p",
    "context": "anywhere"
  },
  {
    "action": "go_to_definition",
    "title": "Go to Definition",
    "key": "f12",
    "context": "diff"
  },
  {
    "action": "peek_definition",
    "title": "Peek Definition",
    "key": "alt+f12",
    "context": "diff"
  },
  {
    "action": "next_change",
    "title": "Next Change",
    "key": "f7",
    "context": "diff"
  },
  {
    "action": "previous_change",
    "title": "Previous Change",
    "key": "shift+f7",
    "context": "diff"
  },
  {
    "action": "next_thread",
    "title": "Next Comment Thread",
    "key": "f8",
    "context": "diff"
  },
  {
    "action": "previous_thread",
    "title": "Previous Comment Thread",
    "key": "shift+f8",
    "context": "diff"
  },
  {
    "action": "find_in_diff",
    "title": "Find in Diff",
    "key": "ctrl+f",
    "context": "diff"
  },
  {
    "action": "toggle_sidebar",
    "title": "Hide or Show the Sidebar",
    "key": "ctrl+b",
    "context": "anywhere"
  },
  {
    "action": "toggle_issue_list",
    "title": "Hide or Show the Issue List",
    "key": "ctrl+alt+b",
    "context": "anywhere"
  },
  {
    "action": "unfold",
    "title": "Unfold an Issue's PRs",
    "key": "arrowright",
    "context": "issue_list"
  },
  {
    "action": "fold",
    "title": "Fold an Issue's PRs",
    "key": "arrowleft",
    "context": "issue_list"
  },
  {
    "action": "toggle_detail_only",
    "title": "Show Only the Detail Pane, or the Panes",
    "key": "ctrl+k z",
    "context": "anywhere"
  },
  {
    "action": "toggle_diff_view",
    "title": "Unified or Side by Side",
    "key": "ctrl+alt+u",
    "context": "diff"
  },
  {
    "action": "toggle_files_mode",
    "title": "All Files or One File at a Time",
    "key": "ctrl+k o",
    "context": "diff"
  },
  {
    "action": "unfold_unchanged",
    "title": "Unfold Unchanged Files",
    "key": "ctrl+k j",
    "context": "diff"
  },
  {
    "action": "next_tab",
    "title": "Next Tab",
    "key": "ctrl+pagedown",
    "context": "anywhere"
  },
  {
    "action": "previous_tab",
    "title": "Previous Tab",
    "key": "ctrl+pageup",
    "context": "anywhere"
  },
  {
    "action": "next_file",
    "title": "Next File",
    "key": "alt+pagedown",
    "context": "diff"
  },
  {
    "action": "previous_file",
    "title": "Previous File",
    "key": "alt+pageup",
    "context": "diff"
  },
  {
    "action": "send_comment",
    "title": "Send Comment",
    "key": "ctrl+enter",
    "context": "composer"
  },
  {
    "action": "open_in_editor",
    "title": "Open in Editor",
    "key": "ctrl+shift+e",
    "context": "diff"
  },
  {
    "action": "local_mark",
    "title": "Local Mark",
    "key": "ctrl+k m",
    "context": "anywhere"
  },
  {
    "action": "pin",
    "title": "Pin or Unpin",
    "key": "ctrl+k p",
    "context": "anywhere"
  },
  {
    "action": "pick_theme",
    "title": "Pick a Theme",
    "key": "ctrl+k t",
    "context": "anywhere"
  },
  {
    "action": "reload_themes",
    "title": "Reload Themes",
    "key": "ctrl+k r",
    "context": "anywhere"
  },
  {
    "action": "go_back",
    "title": "Go Back",
    "key": "alt+arrowleft",
    "context": "anywhere"
  },
  {
    "action": "go_forward",
    "title": "Go Forward",
    "key": "alt+arrowright",
    "context": "anywhere"
  },
  {
    "action": "open_on_github",
    "title": "Open on GitHub",
    "key": "ctrl+shift+g",
    "context": "anywhere"
  },
  {
    "action": "switch_repository",
    "title": "Switch Repository",
    "key": "ctrl+r",
    "context": "anywhere"
  },
  {
    "action": "reset_layout",
    "title": "Reset Layout",
    "key": null,
    "context": "anywhere"
  },
  {
    "action": "archive",
    "title": "Archive",
    "key": "e",
    "context": "issue_list"
  },
  {
    "action": "mute",
    "title": "Mute",
    "key": "m",
    "context": "issue_list"
  },
  {
    "action": "mark_seen",
    "title": "Mark Seen",
    "key": "q",
    "context": "anywhere"
  },
  {
    "action": "mark_unseen",
    "title": "Mark Unseen",
    "key": "shift+q",
    "context": "anywhere"
  },
  {
    "action": "open_settings",
    "title": "Open Settings",
    "key": "ctrl+,",
    "context": "anywhere"
  },
  {
    "action": "zoom_in",
    "title": "Zoom In",
    "key": "ctrl+=",
    "context": "anywhere"
  },
  {
    "action": "zoom_out",
    "title": "Zoom Out",
    "key": "ctrl+-",
    "context": "anywhere"
  },
  {
    "action": "zoom_reset",
    "title": "Reset Zoom",
    "key": "ctrl+0",
    "context": "anywhere"
  },
  {
    "action": "toggle_hud",
    "title": "Show or Hide the Performance HUD",
    "key": "ctrl+shift+h",
    "context": "anywhere"
  },
  {
    "action": "open_devtools",
    "title": "Open DevTools",
    "key": "ctrl+shift+i",
    "context": "anywhere"
  },
  {
    "action": "show_keys",
    "title": "List the View's Keys",
    "key": "?",
    "context": "anywhere"
  }
] as const;

/** What Shift makes each key type on a US keyboard, by what the key types alone. */
export const SHIFTED: Readonly<Record<string, string>> = {
  "'": "\"",
  ",": "<",
  "-": "_",
  ".": ">",
  "/": "?",
  "0": ")",
  "1": "!",
  "2": "@",
  "3": "#",
  "4": "$",
  "5": "%",
  "6": "^",
  "7": "&",
  "8": "*",
  "9": "(",
  ";": ":",
  "=": "+",
  "[": "{",
  "\\": "|",
  "]": "}",
  "`": "~"
};
