# fish config used by the VHS recordings (casts/*.tape).
# Matches the book's Tufte + gruvbox-light palette so the highlighted
# command line visually reads like the book's prose code blocks.

set fish_greeting ""

# Kill autosuggestions — VHS captures them as a flicker of "the next comment"
# one frame ahead of the animated typing. A recording doesn't need them.
set -g fish_autosuggestion_enabled 0

function fish_prompt
    echo -n '$ '
end

# Gruvbox-light-ish syntax colors, aligned with book/src/stylesheets/extra.css.
set -g fish_color_command '#076678'                  # link blue: `shot`, `cd`, etc.
set -g fish_color_param '#3d3333'                    # code text: arguments
set -g fish_color_option '#427b58'                   # accent green: --channel flags
set -g fish_color_quote '#d79921'                    # gruvbox yellow: "strings"
set -g fish_color_comment '#928374'                  # gruvbox gray: # lines
set -g fish_color_operator '#427b58'                 # accent green: pipes/redirects
set -g fish_color_end '#427b58'                      # accent green: ; && ||
set -g fish_color_error '#cc241d'                    # red: invalid commands
set -g fish_color_autosuggestion '#bdb8a8'           # faded cream: inline hints
set -g fish_color_normal '#111111'
