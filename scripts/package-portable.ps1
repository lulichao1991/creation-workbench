$ErrorActionPreference = "Stop"

# Keep this source ASCII-only so Windows PowerShell 5.1 can parse it without a UTF-8 BOM.
$productName = -join ([char[]]@(0x521b, 0x4f5c, 0x5de5, 0x4f5c, 0x53f0))
$portableLabel = -join ([char[]]@(0x514d, 0x5b89, 0x88c5, 0x7248))
$workspace = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$portableRoot = [System.IO.Path]::GetFullPath((Join-Path $workspace "portable"))
$version = (Get-Content -LiteralPath (Join-Path $workspace "package.json") -Raw | ConvertFrom-Json).version
$packageName = "${productName}_${version}_windows_x64_${portableLabel}"
$packageDir = [System.IO.Path]::GetFullPath((Join-Path $portableRoot $packageName))
$zipPath = [System.IO.Path]::GetFullPath((Join-Path $workspace "${packageName}.zip"))
$releaseDir = [System.IO.Path]::GetFullPath((Join-Path $workspace "src-tauri\target\release"))
$application = Join-Path $releaseDir "workbench.exe"
$agentHost = Join-Path $releaseDir "agent-host"

if (-not (Test-Path -LiteralPath $application -PathType Leaf)) { throw "Missing release application: $application" }
if (-not (Test-Path -LiteralPath (Join-Path $agentHost "node.exe") -PathType Leaf)) { throw "Missing bundled Node runtime" }
if (-not (Test-Path -LiteralPath (Join-Path $agentHost "dist\index.js") -PathType Leaf)) { throw "Missing Agent Host" }
if (-not $packageDir.StartsWith($portableRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) { throw "Portable package path escaped its root" }

New-Item -ItemType Directory -Path $portableRoot -Force | Out-Null
if (Test-Path -LiteralPath $packageDir) { Remove-Item -LiteralPath $packageDir -Recurse -Force }
if (Test-Path -LiteralPath $zipPath) { Remove-Item -LiteralPath $zipPath -Force }
New-Item -ItemType Directory -Path $packageDir | Out-Null
Copy-Item -LiteralPath $application -Destination (Join-Path $packageDir "${productName}.exe")
Copy-Item -LiteralPath $agentHost -Destination (Join-Path $packageDir "agent-host") -Recurse

@"
$productName $version Windows x64 portable edition

1. Extract the complete folder, then run ${productName}.exe.
2. Keep the agent-host folder beside the executable; it contains the private Pi SDK runtime.
3. The target computer does not need Pi, Node.js, or npm installed.
4. Before using an Agent, open AI model settings and select a provider/model and enter its API key.
5. Projects are stored in the project root selected on the home screen, not inside this portable folder.
"@ | Set-Content -LiteralPath (Join-Path $packageDir "README.txt") -Encoding UTF8

Compress-Archive -LiteralPath $packageDir -DestinationPath $zipPath -CompressionLevel Optimal
$archive = Get-Item -LiteralPath $zipPath
Write-Output "$($archive.FullName)`t$($archive.Length)"
