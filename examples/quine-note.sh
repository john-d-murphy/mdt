#!/bin/sh
# Make a quine note about the page mdt is showing, and link it back to that page.
#
# Point mdt at this script:
#
#   mdt --note-command ~/Dropbox/GitHub/mdt/examples/quine-note.sh
#
# or, since quine invokes mdt as its pager, in ~/quine.yaml:
#
#   read:
#     pager: mdt --max-width 100 --note-command /path/to/quine-note.sh
#
# mdt right-clicks give it these, in the environment:
#
#   MDT_NOTE_COMMENT    what was typed into the note box
#   MDT_NOTE_SELECTION  the text dragged over, if any
#   MDT_NOTE_REF        the page's quine id, from the metadata under its title
#   MDT_NOTE_FILE       the file being read, empty when it came down a pipe
#   MDT_NOTE_LINE       the line that was right-clicked
#   MDT_NOTE_EDIT       set when mdt has handed over the terminal (ctrl+e), so an editor
#                       can be opened; unset for a note made straight away (ctrl+d)
#   MDT_NOTE_RESULT     a file to write one line into, for mdt's status bar
#
# A line written to $MDT_NOTE_RESULT lands in mdt's status bar. Printing it works too, but
# only when mdt kept the terminal: with the terminal handed over, anything printed is about
# to be drawn over by mdt taking the screen back.
set -eu

# One line for mdt's status bar.
say() {
    if [ -n "${MDT_NOTE_RESULT:-}" ]; then
        printf '%s\n' "$1" >"$MDT_NOTE_RESULT"
    else
        printf '%s\n' "$1"
    fi
}

# A note is the selected passage, what was typed, or both. The passage goes first, because
# `quine new` takes the title from the beginning of the body — so a note made of a passage
# is titled by it. It is not quoted with `>` for the same reason: that would be the title.
if [ -n "${MDT_NOTE_SELECTION:-}" ] && [ -n "${MDT_NOTE_COMMENT:-}" ]; then
    body="$MDT_NOTE_SELECTION

$MDT_NOTE_COMMENT"
elif [ -n "${MDT_NOTE_SELECTION:-}" ]; then
    body=$MDT_NOTE_SELECTION
else
    body=$MDT_NOTE_COMMENT
fi

# `quine new` reads the body on stdin and prints the new note's path.
note=$(printf '%s\n' "$body" | quine new --no-edit)

# With the terminal in hand, open the note itself — front matter and all, the same thing
# `n -e` gives. Not `quine new -e`: its stdin would be the pipe the body came down and its
# stdout the capture above, which leaves an editor with no terminal to work on. The editor
# is pointed at /dev/tty for the same reason, whatever this script's own stdio is. $VISUAL
# may carry arguments (`code -w`), so the path goes in as a positional parameter.
if [ -n "${MDT_NOTE_EDIT:-}" ]; then
    editor=${VISUAL:-${EDITOR:-vi}}
    sh -c "$editor \"\$1\"" sh "$note" </dev/tty >/dev/tty 2>&1
fi

# No id on the page — an ordinary markdown file, or something else piped in — so the note
# stands on its own.
if [ -z "${MDT_NOTE_REF:-}" ]; then
    say "Note made: $(basename "$note")"
    exit 0
fi

# The note is informed by what was being read. Change the relationship here if another one
# fits better; `quine about --relationships` lists the ones that are defined.
#
# `-m` always has something in it: with no -m and a keyboard in reach, `quine edge add` opens
# an editor for the why, which is not wanted here.
why=${MDT_NOTE_COMMENT:-noted while reading}
quine edge add -r informed-by "$note" "$MDT_NOTE_REF" -m "$why" >/dev/null
say "Note made, informed-by $MDT_NOTE_REF: $(basename "$note")"
