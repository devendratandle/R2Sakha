#!/bin/sh
# Install Ardon Studio from this folder. As root it goes to /opt and /usr/share
# for everyone; as a normal user, to your home folder with no admin rights.
# Remove it again with: ./install.sh --uninstall
set -e
here=$(cd "$(dirname "$0")" && pwd)
if [ "$(id -u)" = 0 ]; then
    app=/opt/ardon-studio; share=/usr/share; bin=/usr/bin
else
    app="$HOME/.local/opt/ardon-studio"; share="${XDG_DATA_HOME:-$HOME/.local/share}"; bin="$HOME/.local/bin"
fi
desktop="$share/applications/ardon-studio.desktop"
icon="$share/icons/hicolor/256x256/apps/ardon-studio.png"
mime="$share/mime/packages/ardon-studio.xml"
if [ "$1" = "--uninstall" ]; then
    rm -rf "$app"; rm -f "$desktop" "$icon" "$mime" "$bin/ardon-studio"
    echo "Ardon Studio removed."
else
    mkdir -p "$app" "$bin" "$(dirname "$desktop")" "$(dirname "$icon")" "$(dirname "$mime")"
    cp "$here/ArdonStudio" "$here/r2" "$here/LICENSE" "$here/README.md" "$app/"
    chmod +x "$app/ArdonStudio" "$app/r2"
    cp "$here/ardon-studio.png" "$icon"
    cp "$here/ardon-studio.xml" "$mime"
    sed "s|/opt/ardon-studio|$app|" "$here/ardon-studio.desktop" > "$desktop"
    ln -sf "$app/ArdonStudio" "$bin/ardon-studio"
    echo "Ardon Studio installed in $app. Start it from the applications menu, or run: ardon-studio"
fi
update-mime-database "$share/mime" >/dev/null 2>&1 || true
update-desktop-database "$share/applications" >/dev/null 2>&1 || true
