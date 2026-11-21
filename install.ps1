# Installs the markban CLI and its agent skill (Windows PowerShell 5.1 or newer).
#   irm https://raw.githubusercontent.com/Skediaio/markban/main/install.ps1 | iex
$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$ProgressPreference = 'SilentlyContinue'

$base = 'https://github.com/Skediaio/markban/releases/latest/download'
$target = 'x86_64-pc-windows-msvc'
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\markban'
$tmp = Join-Path ([IO.Path]::GetTempPath()) ('markban-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null

try {
    Write-Host "Downloading markban-$target"
    $zip = Join-Path $tmp 'markban.zip'
    Invoke-WebRequest -UseBasicParsing -Uri "$base/markban-$target.zip" -OutFile $zip
    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    Copy-Item -Path (Join-Path $tmp 'markban.exe') -Destination $installDir -Force

    $skill = Join-Path $tmp 'SKILL.md'
    Invoke-WebRequest -UseBasicParsing -Uri "https://raw.githubusercontent.com/Skediaio/markban/main/skills/markban/SKILL.md" -OutFile $skill
    foreach ($dir in @((Join-Path $HOME '.claude\skills\markban'), (Join-Path $HOME '.config\opencode\skills\markban'))) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
        Copy-Item -Path $skill -Destination (Join-Path $dir 'SKILL.md') -Force
    }
}
finally {
    Remove-Item -Path $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not $userPath) { $userPath = '' }
if (($userPath -split ';') -notcontains $installDir) {
    $newPath = if ($userPath) { "$userPath;$installDir" } else { $installDir }
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    Write-Host "Added $installDir to your user PATH (open a new terminal to pick it up)."
}
if (($env:Path -split ';') -notcontains $installDir) {
    $env:Path = "$env:Path;$installDir"
}

Write-Host ("Installed: " + (& (Join-Path $installDir 'markban.exe') --version))
Write-Host 'Skill installed in ~\.claude\skills\markban and ~\.config\opencode\skills\markban'
