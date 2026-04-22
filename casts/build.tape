Output casts/build.gif

Require shot

Set Shell bash
Set FontSize 16
Set Width 1200
Set Height 560
Set TypingSpeed 30ms
Set PlaybackSpeed 1.0
Set Theme "Catppuccin Mocha"

Set WindowBar Colorful
Set WindowBarSize 40
Set BorderRadius 10
Set Padding 20
Set Margin 20
Set MarginFill "#1e1e2e"

Set LoopOffset 4%

Env PS1 "$ "
Env RUST_LOG "error"

Hide
Type "cd /tmp/shot-demo/lumen-build && clear"
Enter
Show

Sleep 500ms
Type "# Build a local .conda package from the current directory"
Enter
Sleep 300ms

Type "shot build --output-dir ../build-channel"
Enter
# shot build also runs an internal install of build deps (lua + imagemagick),
# which prints its own spinner frames. Hide during that phase.
Hide
Sleep 14s
Show
Sleep 2.5s
