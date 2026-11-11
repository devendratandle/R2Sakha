Ardon Studio for macOS (Apple silicon)

Install: drag "Ardon Studio" onto the Applications folder.

The first time you open it, macOS will refuse, because the app is not yet
signed with an Apple developer certificate. To allow it:

  macOS 15 and later: try to open it once, then open System Settings >
  Privacy & Security, scroll down, and choose "Open Anyway".

  Earlier versions: right-click the app, choose Open, then Open again.

  Or, in Terminal:
      xattr -dr com.apple.quarantine "/Applications/Ardon Studio.app"

You need to do this once only. Ardon-R2, the engine, is inside the app.
