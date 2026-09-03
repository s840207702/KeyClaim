param(
    [switch]$Elevated
)

$ErrorActionPreference = "Stop"
$ruleName = "KeyClaim 局域网键盘召回"
$port = 45823

function Test-IsAdministrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-KeyClaimExecutable {
    $running = Get-CimInstance Win32_Process -Filter "Name = 'keyclaim.exe'" |
        Where-Object { $_.ExecutablePath } |
        Select-Object -First 1
    if ($running) {
        return $running.ExecutablePath
    }

    $candidates = @(
        (Join-Path $env:LOCALAPPDATA "KeyClaim\keyclaim.exe"),
        (Join-Path $env:LOCALAPPDATA "Programs\KeyClaim\keyclaim.exe"),
        (Join-Path $env:ProgramFiles "KeyClaim\keyclaim.exe")
    )
    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) {
            return (Resolve-Path $candidate).Path
        }
    }

    throw "没有找到 keyclaim.exe。请先启动 KeyClaim，再重新运行此脚本。"
}

if (-not (Test-IsAdministrator)) {
    if ($Elevated) {
        throw "Windows 没有授予管理员权限，无法写入防火墙规则。"
    }

    $arguments = @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", ('"{0}"' -f $PSCommandPath),
        "-Elevated"
    )
    $process = Start-Process powershell.exe -Verb RunAs -ArgumentList $arguments -Wait -PassThru
    exit $process.ExitCode
}

$executable = Get-KeyClaimExecutable

Get-NetFirewallRule -DisplayName $ruleName -ErrorAction SilentlyContinue |
    Remove-NetFirewallRule -ErrorAction SilentlyContinue

New-NetFirewallRule `
    -DisplayName $ruleName `
    -Description "允许 KeyClaim 在本地子网内接收键盘召回消息" `
    -Direction Inbound `
    -Action Allow `
    -Enabled True `
    -Profile Any `
    -Protocol UDP `
    -LocalPort $port `
    -RemoteAddress LocalSubnet | Out-Null

Get-Process keyclaim -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500
Start-Process -FilePath $executable
Start-Sleep -Seconds 2

$listener = Get-NetUDPEndpoint -LocalPort $port -ErrorAction SilentlyContinue |
    Where-Object { $_.OwningProcess -in (Get-Process keyclaim -ErrorAction SilentlyContinue).Id } |
    Select-Object -First 1
$rule = Get-NetFirewallRule -DisplayName $ruleName -ErrorAction SilentlyContinue |
    Select-Object -First 1

$result = [ordered]@{
    repaired = [bool]($rule -and $listener)
    executable = $executable
    firewallRule = if ($rule) { $rule.DisplayName } else { $null }
    firewallEnabled = if ($rule) { $rule.Enabled.ToString() } else { $null }
    firewallAction = if ($rule) { $rule.Action.ToString() } else { $null }
    firewallScope = "LocalSubnet UDP/$port (all KeyClaim install paths)"
    udp45823Listening = [bool]$listener
    processId = if ($listener) { $listener.OwningProcess } else { $null }
}

$result | ConvertTo-Json -Depth 3

if (-not $result.repaired) {
    throw "防火墙规则或 UDP 监听仍未就绪，请运行 diagnose-windows.ps1 获取只读诊断。"
}
