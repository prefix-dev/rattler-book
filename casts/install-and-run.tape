Output casts/install-and-run.gif

Require shot

Set Shell bash
Set FontSize 16
Set Width 1200
Set Height 720
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
Type "cd /tmp/shot-demo/work && clear"
Enter
Show

Sleep 500ms
Type "# Create a new project"
Enter
Sleep 300ms

Type "shot init my-app --channel ../channel --channel conda-forge"
Enter
Sleep 1.2s

Type "# Add any conda dependency"
Enter
Sleep 300ms

Type "shot add lumen"
Enter
Sleep 1.2s

Type "# Install your environment"
Enter
Sleep 300ms

Type "shot install"
Enter
# Skip the spinner churn (~10–12 s of cache-validate / extract / link
# frames); jump straight to the completed install output.
Hide
Sleep 14s
Show
Sleep 2s

Type "# Run a simple lua script in an isolated environment"
Enter
Sleep 300ms

Type `shot run lua -e "require('lumen').thumbnail('photo.jpg', 128)"`
Enter
Sleep 2.5s
