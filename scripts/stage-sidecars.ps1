param(
  [string] $YtDlpPath = "",
  [string] $FfmpegPath = "",
  [string] $FfprobePath = ""
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$targetDir = Join-Path $repoRoot "src-tauri\binaries"
$targetTriple = "x86_64-pc-windows-msvc"

New-Item -ItemType Directory -Force -Path $targetDir | Out-Null

function Resolve-ToolPath {
  param(
    [string] $ExplicitPath,
    [string] $CommandName
  )

  if ($ExplicitPath) {
    $resolved = Resolve-Path -LiteralPath $ExplicitPath
    return $resolved.Path
  }

  $command = Get-Command $CommandName -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $command) {
    throw "Could not find $CommandName. Pass -$($CommandName.Replace('-', ''))Path explicitly."
  }

  return $command.Source
}

function Copy-Sidecar {
  param(
    [string] $SourcePath,
    [string] $BaseName
  )

  $destination = Join-Path $targetDir "$BaseName-$targetTriple.exe"
  Copy-Item -LiteralPath $SourcePath -Destination $destination -Force
  $sizeMb = [Math]::Round((Get-Item -LiteralPath $destination).Length / 1MB, 2)
  Write-Host "Staged $BaseName -> $destination ($sizeMb MB)"
}

Copy-Sidecar -SourcePath (Resolve-ToolPath $YtDlpPath "yt-dlp") -BaseName "yt-dlp"
Copy-Sidecar -SourcePath (Resolve-ToolPath $FfmpegPath "ffmpeg") -BaseName "ffmpeg"
Copy-Sidecar -SourcePath (Resolve-ToolPath $FfprobePath "ffprobe") -BaseName "ffprobe"
