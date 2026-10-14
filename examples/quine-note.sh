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
#
# Whatever this prints lands in mdt's status bar.
set -eu

# The comment is the body; anything selected goes under it as a quotation.
body=$MDT_NOTE_COMMENT
if [ -n "${MDT_NOTE_SELECTION:-}" ]; then
    body="$MDT_NOTE_COMMENT

$(printf '%s\n' "$MDT_NOTE_SELECTION" | sed 's/^/> /')"
fi

# `quine new` reads the body on stdin and prints the new note's path.
note=$(printf '%s\n' "$body" | quine new --no-edit)

# No id on the page — an ordinary markdown file, or something else piped in — so the note
# stands on its own.
if [ -z "${MDT_NOTE_REF:-}" ]; then
    echo "Note made: $(basename "$note")"
    exit 0
fi

# The note is informed by what was being read. Change the relationship here if another one
# fits better; `quine about --relationships` lists the ones that are defined.
quine edge add -r informed-by "$note" "$MDT_NOTE_REF" -m "$MDT_NOTE_COMMENT" >/dev/null
echo "Note made, informed-by $MDT_NOTE_REF: $(basename "$note")"
