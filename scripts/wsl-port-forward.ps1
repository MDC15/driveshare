param(
    [int]$Port = 8080,
    [switch]$Remove
)

$ErrorActionPreference = "Stop"

function Get-WslIp {
    $result = wsl -- ip -4 addr show eth0 | Select-String -Pattern 'inet\s+(\d+\.\d+\.\d+\.\d+)'
    if ($result) {
        return $result.Matches.Groups[1].Value
    }
    throw "Cannot get WSL2 IP address"
}

$wslIp = Get-WslIp
Write-Host "WSL2 IP: $wslIp" -ForegroundColor Cyan

if ($Remove) {
    Write-Host "Removing portproxy rules..." -ForegroundColor Yellow
    netsh interface portproxy delete v4tov4 listenport=$Port listenaddress=0.0.0.0
    netsh interface portproxy delete v4tov4 listenport=$Port listenaddress=127.0.0.1

    Write-Host "Removing firewall rule..." -ForegroundColor Yellow
    netsh advfirewall firewall delete rule name="DriveShare $Port"

    Write-Host "Current portproxy rules:" -ForegroundColor Cyan
    netsh interface portproxy show all
    return
}

Write-Host "Setting up port forwarding: Windows :$Port -> WSL2 ($wslIp):$Port" -ForegroundColor Green

netsh interface portproxy add v4tov4 listenaddress=0.0.0.0 listenport=$Port connectaddress=$wslIp connectport=$Port
netsh interface portproxy add v4tov4 listenaddress=127.0.0.1 listenport=$Port connectaddress=$wslIp connectport=$Port

Write-Host "Adding Windows Firewall rule..." -ForegroundColor Green
netsh advfirewall firewall add rule name="DriveShare $Port" dir=in action=allow protocol=TCP localport=$Port

Write-Host "Verifying portproxy rules:" -ForegroundColor Cyan
netsh interface portproxy show all

$winIp = (Get-NetIPAddress -AddressFamily IPv4 | Where-Object {
    $_.InterfaceAlias -notlike '*Loopback*' -and
    $_.InterfaceAlias -notlike '*vEthernet*' -and
    $_.PrefixOrigin -ne 'WellKnown'
}).IPAddress | Select-Object -First 1

Write-Host ""
Write-Host "=== DONE ===" -ForegroundColor Green
Write-Host "Access from other devices: http://$winIp`:$Port" -ForegroundColor Green
Write-Host "Access from this machine:  http://localhost:$Port" -ForegroundColor Green
