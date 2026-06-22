# README hero screenshot (docs/screenshot.png).
# Driven by demo/hero-screenshot.sh, which first stands up an ISOLATED tmux
# server and the demo sessions. VHS runs with cwd = demo/, so the relative
# Screenshot path below lands in docs/.
Output "hero-dummy.mp4"
Set Theme { "background": "#191724", "foreground": "#e0def4", "black": "#26233a", "red": "#eb6f92", "green": "#31748f", "yellow": "#f6c177", "blue": "#9ccfd8", "magenta": "#c4a7e7", "cyan": "#ebbcba", "white": "#e0def4", "brightBlack": "#6e6a86", "brightRed": "#eb6f92", "brightGreen": "#31748f", "brightYellow": "#f6c177", "brightBlue": "#9ccfd8", "brightMagenta": "#c4a7e7", "brightCyan": "#ebbcba", "brightWhite": "#e0def4", "cursor": "#e0def4", "selection": "#403d52" }
Set FontSize 26
Set Width 1040
Set Height 820
Set Padding 0
Set TypingSpeed 25ms
Set Shell "bash"
Hide
Type "clear"
Enter
Sleep 200ms
Show
Sleep 400ms
Type "ttree"
Enter
Sleep 2800ms
Enter
Sleep 3400ms
Screenshot "../docs/screenshot.png"
Ctrl+p
Sleep 300ms
Type "q"
Sleep 400ms
