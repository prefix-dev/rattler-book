Output casts/build.gif

Require shot
Require fish

Set Shell fish
Set FontFamily "IBM Plex Mono"
Set FontSize 18
Set Width 1200
Set Height 560
Set TypingSpeed 30ms
Set PlaybackSpeed 1.0

Set Theme { "background": "#fffff8", "foreground": "#111111", "cursor": "#427b58", "selection": "#427b581a", "black": "#111111", "red": "#cc241d", "green": "#427b58", "yellow": "#d79921", "blue": "#076678", "magenta": "#b16286", "cyan": "#689d6a", "white": "#f7f5ef", "brightBlack": "#928374", "brightRed": "#cc241d", "brightGreen": "#98971a", "brightYellow": "#fabd2f", "brightBlue": "#458588", "brightMagenta": "#8f3f71", "brightCyan": "#689d6a", "brightWhite": "#fffff8" }

Set CursorBlink true

Set WindowBar Colorful
Set WindowBarSize 40
Set BorderRadius 12
Set Padding 24
Set Margin 40
Set MarginFill "casts/bg-build.png"

Set WaitPattern /\$$/
Set WaitTimeout 30s

Set LoopOffset 4%

Env RUST_LOG "error"

Hide
Type "source /tmp/shot-demo/fish-config/fish/config.fish; cd /tmp/shot-demo/lumen-build; clear"
Enter
Wait
Show

Sleep 1s
Type "# Build a local .conda package from the current directory"
Enter
Sleep 700ms

Type "shot build --output-dir ../build-channel"
Enter
Wait
Sleep 2s
