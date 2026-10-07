#!/bin/sh
# Collects what decides how the Lume Orb behaves on Linux (focus and size problems).
# Run it with Lume open and the Orb expanded; with a text box ("Renomear"/"Continuar")
# open and the terminal that stole the typing behind it, if you can.
#   sh scripts/diagnose-orb-linux.sh > orb-diagnostico.txt
echo "== sessão"
echo "XDG_SESSION_TYPE=$XDG_SESSION_TYPE XDG_CURRENT_DESKTOP=$XDG_CURRENT_DESKTOP DISPLAY=$DISPLAY"
command -v lsb_release >/dev/null 2>&1 && lsb_release -d
command -v gsettings >/dev/null 2>&1 && {
  echo "fractional scaling: $(gsettings get org.gnome.mutter experimental-features 2>/dev/null)"
  echo "scaling-factor: $(gsettings get org.gnome.desktop.interface scaling-factor 2>/dev/null)"
}

echo "== Xwayland e bibliotecas"
echo "Xwayland rodando: $(pgrep -a Xwayland 2>/dev/null | head -1 || true)"
echo "binário Xwayland: $(command -v Xwayland 2>/dev/null || echo ausente)"
if command -v ldconfig >/dev/null 2>&1; then
  ldconfig -p 2>/dev/null | grep -E "libgtk-layer-shell|libwebkit2gtk-4.1|libgtk-3\.so|libX11\.so" | sed 's/^ *//'
fi
echo "Modo escolhido pelo Lume: com DISPLAY definido o Lume usa o XWayland (ele imprime"
echo "'usando XWayland...' ao iniciar); sem DISPLAY usa o Wayland nativo, limitado."

if ! command -v xwininfo >/dev/null 2>&1 || ! command -v xprop >/dev/null 2>&1; then
  echo "== xwininfo/xprop ausentes: instale o pacote x11-utils e rode de novo"
  exit 0
fi

echo "== janela ativa"
xprop -root _NET_ACTIVE_WINDOW 2>&1

echo "== janelas X do Lume"
xwininfo -root -tree 2>/dev/null | grep -i "lume" | while read -r line; do
  id=$(echo "$line" | awk '{print $1}')
  echo "-- $line"
  xwininfo -id "$id" 2>/dev/null | grep -E "Absolute upper-left|Width|Height|Map State|Override Redirect"
  xprop -id "$id" _NET_WM_WINDOW_TYPE _NET_WM_STATE WM_HINTS WM_PROTOCOLS \
    _NET_WM_ALLOWED_ACTIONS WM_NORMAL_HINTS _GTK_FRAME_EXTENTS 2>&1 | grep -v "not found"
done
