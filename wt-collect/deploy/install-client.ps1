param(
    [Parameter(Mandatory=$true)][string]$ZipPath,
    [string]$InstallRoot = (Join-Path $env:USERPROFILE 'WT-Tactical-Command\2.0.0')
)
$ErrorActionPreference = 'Stop'
if ([Diagnostics.Process]::GetCurrentProcess().SessionId -ne 0) {
    throw 'Use a noninteractive background session for deployment.'
}
if (Test-Path $InstallRoot) { throw 'Release directory already exists; inspect before replacing.' }
$stageRoot = $InstallRoot + '.staging'
if (Test-Path $stageRoot) { throw 'Staging directory already exists.' }
New-Item -ItemType Directory -Force $stageRoot | Out-Null
Expand-Archive -LiteralPath $ZipPath -DestinationPath $stageRoot
$source = Join-Path $stageRoot 'wt-collector'
$exe = Join-Path $source 'wt-collector.exe'
$config = Join-Path $source 'wt-collector.toml'
if (!(Test-Path $exe) -or !(Test-Path $config)) { throw 'Incomplete collector package.' }
$text = [IO.File]::ReadAllText($config)
if ($text.Contains('CHANGE_ME_WT_TOKEN')) { throw 'This machine requires the privately configured package.' }
if ($text -notmatch 'always_on_top\s*=\s*false') { throw 'Always-on-top must be disabled.' }
$identity = [Security.Principal.WindowsIdentity]::GetCurrent().User
$acl = New-Object Security.AccessControl.FileSecurity
$acl.SetOwner($identity)
$acl.SetAccessRuleProtection($true, $false)
foreach ($sid in @($identity.Value, 'S-1-5-18', 'S-1-5-32-544')) {
    $principal = New-Object Security.Principal.SecurityIdentifier($sid)
    $rule = New-Object Security.AccessControl.FileSystemAccessRule($principal, 'FullControl', 'Allow')
    $acl.AddAccessRule($rule)
}
Set-Acl -LiteralPath $config -AclObject $acl
$version = & $exe --version
if ($LASTEXITCODE -ne 0 -or $version -ne 'wt-collector 2.0.0') { throw 'Unexpected executable version.' }
Move-Item -LiteralPath $source -Destination $InstallRoot
Remove-Item -LiteralPath $stageRoot -Recurse
$shell = New-Object -ComObject WScript.Shell
$shortcutPath = Join-Path ([Environment]::GetFolderPath('Desktop')) 'WT Tactical Command 2.0.lnk'
$shortcut = $shell.CreateShortcut($shortcutPath)
$shortcut.TargetPath = Join-Path $InstallRoot 'wt-collector.exe'
$shortcut.WorkingDirectory = $InstallRoot
$shortcut.IconLocation = $shortcut.TargetPath + ',0'
$shortcut.Save()
Remove-Item -LiteralPath $ZipPath
@{ installed=$InstallRoot; version=$version; session=0; started=$false;
   shortcut=$shortcutPath; exeSha256=(Get-FileHash (Join-Path $InstallRoot 'wt-collector.exe') -Algorithm SHA256).Hash } |
    ConvertTo-Json -Compress
