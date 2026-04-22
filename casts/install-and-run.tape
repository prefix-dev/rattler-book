Output casts/install-and-run.gif

Require shot
Require chafa
Require fish

Set Shell fish
Set FontFamily "IBM Plex Mono"
Set FontSize 18
Set Width 1200
Set Height 900
Set TypingSpeed 30ms
Set PlaybackSpeed 1.0

# Book palette: Tufte cream bg + gruvbox-light accents (see book/src/stylesheets/extra.css).
Set Theme { "background": "#fffff8", "foreground": "#111111", "cursor": "#427b58", "selection": "#427b581a", "black": "#111111", "red": "#cc241d", "green": "#427b58", "yellow": "#d79921", "blue": "#076678", "magenta": "#b16286", "cyan": "#689d6a", "white": "#f7f5ef", "brightBlack": "#928374", "brightRed": "#cc241d", "brightGreen": "#98971a", "brightYellow": "#fabd2f", "brightBlue": "#458588", "brightMagenta": "#8f3f71", "brightCyan": "#689d6a", "brightWhite": "#fffff8" }

Set CursorBlink true

Set WindowBar Colorful
Set WindowBarSize 40
Set BorderRadius 12
Set Padding 24
Set Margin 40
Set MarginFill "casts/bg-install.png"

# Wait matches fish's "$" prompt at end of line — use instead of Sleep so
# typing never starts before the previous command has finished.
Set WaitPattern /\$$/
Set WaitTimeout 30s

Set LoopOffset 4%

Env RUST_LOG "error"

Hide
Type "source /tmp/shot-demo/fish-config/fish/config.fish; cd /tmp/shot-demo/work; clear"
Enter
Wait
Show

Sleep 1s
Type "# Create a new project"
Enter
Sleep 700ms

Type "shot init my-app --channel ../channel --channel conda-forge"
Enter
Wait

Enter
Wait
Sleep 700ms

Type "# Add any conda dependency"
Enter
Sleep 700ms

Type "shot add lumen"
Enter
Wait

Enter
Wait
Sleep 700ms

Type "# Install your environment"
Enter
Sleep 700ms

Type "shot install"
Enter
Wait

Enter
Wait

Type "# Now that we have our environment set up, let's thumbnail the photo"
Enter
Sleep 700ms

Type "# ...with lumen, the library we just installed, at 128px"
Enter
Type "# We'll use the run command. Lets go!"
Enter
Sleep 700ms

Type@15ms `shot run lua -e "require('lumen').thumbnail('photo.jpg', 128)"`
Enter
Wait

Enter
Wait

Type "# So, lets see how that turned out"
Enter
Sleep 700ms

Type@15ms "chafa --format=symbols --size=40x20 photo_thumb.jpg"
Enter
Wait
Sleep 2s
