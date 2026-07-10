#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
VT=${1:-3}

echo "▸ Compilando atlas-core com backend udev..."
cargo build -p compositor 2>&1

echo "▸ Verificando seatd..."
if ! pgrep -x seatd >/dev/null 2>&1; then
    echo "⚠ seatd não está rodando. Tentando iniciar..."
    sudo seatd -g "$(id -g)" -u "$(id -u)" -d &
    sleep 1
fi

echo "▸ Executando Atlas no VT $VT via openvt..."
echo "  Pressione Ctrl+Alt+F${VT} para ver o Atlas rodando."
echo "  Pressione Ctrl+Alt+F1 (ou F2) para voltar ao Hyprland."
echo "  Pressione Ctrl+C aqui para encerrar."
echo ""

sudo openvt -c "$VT" -s -e "$ROOT_DIR/target/debug/compositor" -- --tty-udev
