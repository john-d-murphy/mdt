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
#
# Whatever this prints lands in mdt's status bar.
set -eu

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

# `quine new` reads the body on stdin and prints the new note's path. With the terminal in
# hand, `-e` opens $VISUAL/$EDITOR on the note itself — front matter and all, the same thing
# `n -e` gives — and what is saved is the note.
if [ -n "${MDT_NOTE_EDIT:-}" ]; then
    note=$(printf '%s\n' "$body" | quine new -e)
else
    note=$(printf '%s\n' "$body" | quine new --no-edit)
fi

# No id on the page — an ordinary markdown file, or something else piped in — so the note
# stands on its own.
if [ -z "${MDT_NOTE_REF:-}" ]; then
    echo "Note made: $(basename "$note")"
    exit 0
fi

# The note is informed by what was being read. Change the relationship here if another one
# fits better; `quine about --relationships` lists the ones that are defined.
#
# `-m` always has something in it: with no -m and a keyboard in reach, `quine edge add` opens
# an editor for the why, which is not wanted here.
why=${MDT_NOTE_COMMENT:-noted while reading}
quine edge add -r informed-by "$note" "$MDT_NOTE_REF" -m "$why" >/dev/null
echo "Note made, informed-by $MDT_NOTE_REF: $(basename "$note")"
