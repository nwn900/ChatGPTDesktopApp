param([int]$Start = 0)
$ErrorActionPreference = 'Stop'
$env:CC='cl.exe'
$env:CXX='cl.exe'
$env:CARGO_TARGET_DIR='E:\desktop-app-suite-build'
$env:CARGO_PROFILE_DEV_DEBUG='0'
$env:CARGO_PROFILE_TEST_DEBUG='0'
$env:CARGO_INCREMENTAL='0'
$env:CARGO_BUILD_JOBS='2'
$taskRoots=@(
 'C:\Users\micha\Desktop\Projects\ChatGPT Web App',
 'C:\Users\micha\Desktop\Projects\GeminiWindowsApp',
 'C:\Users\micha\Desktop\Projects\GrokWindowsApp',
 'C:\Users\micha\Desktop\Projects\ClaudeWindowsApp',
 'C:\Users\micha\Desktop\Projects\YT Music Listener App\yt-music-desktop'
)
for($taskIndex=$Start;$taskIndex -lt $taskRoots.Count;$taskIndex++){
 $taskRoot=$taskRoots[$taskIndex]
 Push-Location (Join-Path $taskRoot 'src-tauri')
 try {
  Write-Output "Testing $taskRoot"
  & 'C:\Users\micha\.cargo\bin\cargo.exe' fmt
  & 'C:\Users\micha\.cargo\bin\cargo.exe' test -j 2
  if($LASTEXITCODE -ne 0){throw "Tests failed: $taskRoot"}
  Write-Output "Building $taskRoot"
  & 'C:\Users\micha\.cargo\bin\cargo.exe' tauri build -- --locked
  if($LASTEXITCODE -ne 0){throw "Build failed: $taskRoot"}
  $taskConfig=Get-Content -Raw 'tauri.conf.json' | ConvertFrom-Json
  $taskRelease=Join-Path $taskRoot ('releases\v'+$taskConfig.version)
  New-Item -ItemType Directory -Force -Path $taskRelease | Out-Null
  $taskInstaller=Join-Path $env:CARGO_TARGET_DIR ('release\bundle\nsis\'+$taskConfig.productName+'_'+$taskConfig.version+'_x64-setup.exe')
  Copy-Item -LiteralPath $taskInstaller -Destination $taskRelease
  $taskStream=[System.IO.File]::OpenRead($taskInstaller)
  try {
   $taskHasher=[System.Security.Cryptography.SHA256]::Create()
   try { Write-Output ([BitConverter]::ToString($taskHasher.ComputeHash($taskStream)).Replace('-','')) }
   finally { $taskHasher.Dispose() }
  } finally { $taskStream.Dispose() }
 } finally { Pop-Location }
}
