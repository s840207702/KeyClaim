$ErrorActionPreference = "SilentlyContinue"

$port = 45823
$processes = @(Get-CimInstance Win32_Process -Filter "Name = 'keyclaim.exe'" | ForEach-Object {
    [ordered]@{
        pid = $_.ProcessId
        executable = $_.ExecutablePath
        commandLine = $_.CommandLine
    }
})

$listeners = @(Get-NetUDPEndpoint -LocalPort $port | ForEach-Object {
    [ordered]@{
        address = $_.LocalAddress
        port = $_.LocalPort
        owningProcess = $_.OwningProcess
    }
})

$firewallRules = @(Get-NetFirewallRule | Where-Object {
    $_.DisplayName -match "KeyClaim" -or $_.Description -match "KeyClaim"
} | ForEach-Object {
    [ordered]@{
        name = $_.DisplayName
        enabled = $_.Enabled.ToString()
        direction = $_.Direction.ToString()
        action = $_.Action.ToString()
        profile = $_.Profile.ToString()
    }
})

$configPath = Join-Path $env:APPDATA "com.feige.keyclaim\keyclaim-settings.json"
$config = $null
if (Test-Path $configPath) {
    $stored = Get-Content $configPath -Raw | ConvertFrom-Json
    if ($stored.config) {
        $config = [ordered]@{
            hostName = $stored.config.hostName
            platform = $stored.config.platform
            channel = $stored.config.channel
            peerChannel = $stored.config.peerChannel
            networkPort = $stored.config.networkPort
            autoRecall = $stored.config.autoRecall
            launchAtLogin = $stored.config.launchAtLogin
            pairingCodeConfigured = [bool]$stored.config.pairingCode
        }
    }
}

$logitechDevices = @(Get-PnpDevice -PresentOnly | Where-Object {
    $_.InstanceId -match "VID_046D" -and ($_.Class -eq "HIDClass" -or $_.FriendlyName -match "Logitech|罗技")
} | Select-Object -First 30 | ForEach-Object {
    [ordered]@{
        status = $_.Status
        class = $_.Class
        name = $_.FriendlyName
        instanceId = $_.InstanceId
    }
})

$result = [ordered]@{
    computer = $env:COMPUTERNAME
    timestamp = (Get-Date).ToString("o")
    keyclaimProcesses = $processes
    udp45823Listeners = $listeners
    keyclaimFirewallRules = $firewallRules
    configPath = $configPath
    configExists = Test-Path $configPath
    config = $config
    logitechHidDevices = $logitechDevices
    ipv4 = @(Get-NetIPAddress -AddressFamily IPv4 | Where-Object {
        $_.IPAddress -notlike "127.*" -and $_.IPAddress -notlike "169.254.*"
    } | ForEach-Object {
        [ordered]@{
            interface = $_.InterfaceAlias
            address = $_.IPAddress
            prefixLength = $_.PrefixLength
        }
    })
}

$result | ConvertTo-Json -Depth 6
