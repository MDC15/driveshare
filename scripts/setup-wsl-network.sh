#!/bin/bash
set -e

PORT="${1:-8443}"

echo "DriveShare WSL2 Network Setup"
echo "============================="
echo "Port: $PORT"
echo ""

# Kill any existing driveshare
pkill driveshare 2>/dev/null || true
sleep 1

# Start server in background
echo "[1] Starting DriveShare server..."
./target/release/driveshare --port "$PORT" &
sleep 2

WSL_IP=$(ip -4 addr show eth0 | grep -oP '(?<=inet\s)\d+(\.\d+){3}')
echo "[2] WSL2 IP: $WSL_IP"

# Run PowerShell setup on Windows
echo "[3] Setting up Windows port forwarding & firewall..."
powershell.exe -Command "Start-Process PowerShell -Verb RunAs -ArgumentList '-NoProfile -ExecutionPolicy Bypass -Command \"
    netsh interface portproxy delete v4tov4 listenport=$PORT listenaddress=0.0.0.0 2>null;
    netsh interface portproxy add v4tov4 listenaddress=0.0.0.0 listenport=$PORT connectaddress=$WSL_IP connectport=$PORT;
    netsh interface portproxy add v4tov4 listenaddress=127.0.0.1 listenport=$PORT connectaddress=$WSL_IP connectport=$PORT;
    netsh advfirewall firewall add rule name=\\"DriveShare $PORT\\" dir=in action=allow protocol=TCP localport=$PORT;
    netsh interface portproxy show all;
    Write-Host 'DONE - Port forwarding active for :$PORT';
    pause
\"" 2>/dev/null || echo "Could not auto-run PowerShell. Run scripts/wsl-port-forward.ps1 manually as Administrator."

echo ""
WIN_IP=$(powershell.exe -Command "ipconfig | findstr IPv4" 2>/dev/null | head -1 | grep -oP '\d+\.\d+\.\d+\.\d+')
echo "Access from other devices: http://$WIN_IP:$PORT"
echo "Access locally:           http://localhost:$PORT"