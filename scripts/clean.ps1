#requires -Version 5.1
<#
.SYNOPSIS
    Reclaim disk space by deleting Lashon's build artifacts and (optionally)
    language environments and downloaded model weights.

.DESCRIPTION
    Three tiers, from safest to most aggressive:

      light       Cargo target/ + empty orphan worktree dirs.
                  Cost of recovery: one cold cargo build.

      medium      light + Python .venv, PyInstaller build/dist, npm
                  node_modules, .svelte-kit, Tauri-side bundled binaries
                  (regenerated at packaging time).
                  Cost of recovery: npm install, recreate venv, re-mirror
                  llama-server binaries from the ggml.llamacpp release.

      aggressive  medium + models/stt + models/local-llm.
                  Cost of recovery: multi-GB downloads on next run.

    What stays untouched at every tier:
      - tracked files (the script never touches anything under git ls-files);
      - committed model files (models/wake/wakewords/hey_lashon.onnx,
        models/wake/openwakeword/*, models/vad/silero-vad-v5/*);
      - registered git worktrees (only zero-byte orphan dirs are pruned);
      - secrets (.env files are never touched — they are gitignored,
        but the script does not reach for them either way).

    See .claude/rules/cleanup.md for the routine and tier policy.

.PARAMETER Level
    Cleanup tier: light (default), medium, or aggressive.

.PARAMETER DryRun
    Show what would be deleted and the projected reclaim, without touching
    anything.

.EXAMPLE
    .\scripts\clean.ps1
    Light tier — the safe default.

.EXAMPLE
    .\scripts\clean.ps1 -Level medium -DryRun
    Preview a medium-tier sweep without deleting.

.EXAMPLE
    .\scripts\clean.ps1 -Level aggressive
    Reclaim everything regenerable, including downloaded model weights.
#>
[CmdletBinding()]
param(
    [ValidateSet('light', 'medium', 'aggressive')]
    [string]$Level = 'light',

    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'

function Get-RepoRoot {
    $root = git rev-parse --show-toplevel 2>$null
    if (-not $root) { throw "Not inside a git repository." }
    return (Resolve-Path $root).Path
}

function Get-DirSizeMB {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return 0 }
    $bytes = (Get-ChildItem -LiteralPath $Path -Recurse -Force -ErrorAction SilentlyContinue |
              Measure-Object -Property Length -Sum).Sum
    if (-not $bytes) { return 0 }
    return [math]::Round($bytes / 1MB, 1)
}

function Get-RepoTotalMB {
    param([string]$Root)
    $bytes = (Get-ChildItem -LiteralPath $Root -Recurse -Force -ErrorAction SilentlyContinue |
              Where-Object { -not $_.PSIsContainer } |
              Measure-Object -Property Length -Sum).Sum
    if (-not $bytes) { return 0 }
    return [math]::Round($bytes / 1MB, 1)
}

function Remove-PathSafely {
    param(
        [string]$Path,
        [string]$Label,
        [switch]$DryRun
    )
    if (-not (Test-Path -LiteralPath $Path)) {
        Write-Host ("  - {0,-55} (absent)" -f $Label) -ForegroundColor DarkGray
        return 0
    }
    $sizeMB = Get-DirSizeMB -Path $Path
    if ($DryRun) {
        Write-Host ("  - {0,-55} {1,8} MB  (dry-run)" -f $Label, $sizeMB) -ForegroundColor Yellow
    }
    else {
        Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction SilentlyContinue
        Write-Host ("  - {0,-55} {1,8} MB  deleted" -f $Label, $sizeMB) -ForegroundColor Green
    }
    return $sizeMB
}

function Remove-OrphanWorktreeDirs {
    param(
        [string]$Root,
        [switch]$DryRun
    )
    $worktreeDir = Join-Path $Root '.claude\worktrees'
    if (-not (Test-Path $worktreeDir)) { return 0 }

    $registered = @()
    foreach ($line in (git worktree list --porcelain)) {
        if ($line -like 'worktree *') {
            $registered += ($line -replace '^worktree ', '').Replace('/', '\').TrimEnd('\')
        }
    }

    $reclaimed = 0
    foreach ($dir in Get-ChildItem -LiteralPath $worktreeDir -Directory -Force) {
        $full = $dir.FullName.TrimEnd('\')
        if ($registered -contains $full) { continue }
        $sizeMB = Get-DirSizeMB -Path $full
        if ($DryRun) {
            Write-Host ("  - orphan worktree {0,-39} {1,8} MB  (dry-run)" -f $dir.Name, $sizeMB) -ForegroundColor Yellow
        }
        else {
            Remove-Item -LiteralPath $full -Recurse -Force -ErrorAction SilentlyContinue
            Write-Host ("  - orphan worktree {0,-39} {1,8} MB  deleted" -f $dir.Name, $sizeMB) -ForegroundColor Green
        }
        $reclaimed += $sizeMB
    }

    if (-not $DryRun) { git worktree prune | Out-Null }
    return $reclaimed
}

$root = Get-RepoRoot
Push-Location $root
try {
    $beforeMB = Get-RepoTotalMB -Root $root
    $beforeGB = [math]::Round($beforeMB / 1024, 2)
    Write-Host ""
    Write-Host "Lashon repo cleanup -- tier: $Level $(if ($DryRun) { '(dry-run)' })" -ForegroundColor Cyan
    Write-Host ("Repo total before: {0} GB" -f $beforeGB)
    Write-Host ""

    $reclaimed = 0

    Write-Host "Build artefacts:" -ForegroundColor Cyan
    if ($DryRun) {
        $targetMB = Get-DirSizeMB -Path (Join-Path $root 'target')
        Write-Host ("  - target/ (cargo clean)                                  {0,8} MB  (dry-run)" -f $targetMB) -ForegroundColor Yellow
        $reclaimed += $targetMB
    }
    else {
        if (Test-Path (Join-Path $root 'target')) {
            $targetMB = Get-DirSizeMB -Path (Join-Path $root 'target')
            cargo clean 2>&1 | Out-Null
            Write-Host ("  - target/ (cargo clean)                                  {0,8} MB  deleted" -f $targetMB) -ForegroundColor Green
            $reclaimed += $targetMB
        }
        else {
            Write-Host "  - target/ (cargo clean)                                  (absent)" -ForegroundColor DarkGray
        }
    }
    $reclaimed += Remove-OrphanWorktreeDirs -Root $root -DryRun:$DryRun

    if ($Level -eq 'medium' -or $Level -eq 'aggressive') {
        Write-Host ""
        Write-Host "Language environments and regenerated bundles:" -ForegroundColor Cyan
        $reclaimed += Remove-PathSafely -Path (Join-Path $root 'services\stt-sidecar\.venv') -Label 'services/stt-sidecar/.venv' -DryRun:$DryRun
        $reclaimed += Remove-PathSafely -Path (Join-Path $root 'services\stt-sidecar\build') -Label 'services/stt-sidecar/build' -DryRun:$DryRun
        $reclaimed += Remove-PathSafely -Path (Join-Path $root 'services\stt-sidecar\dist')  -Label 'services/stt-sidecar/dist'  -DryRun:$DryRun
        $reclaimed += Remove-PathSafely -Path (Join-Path $root 'apps\desktop\node_modules')  -Label 'apps/desktop/node_modules'  -DryRun:$DryRun
        $reclaimed += Remove-PathSafely -Path (Join-Path $root 'apps\desktop\.svelte-kit')   -Label 'apps/desktop/.svelte-kit'   -DryRun:$DryRun
        $reclaimed += Remove-PathSafely -Path (Join-Path $root 'apps\desktop\build')         -Label 'apps/desktop/build'         -DryRun:$DryRun

        # PyInstaller-frozen sidecar — kept .gitkeep, blow away everything else.
        $sttBin = Join-Path $root 'apps\desktop\src-tauri\binaries\lashon-stt'
        if (Test-Path $sttBin) {
            $items = Get-ChildItem -LiteralPath $sttBin -Force | Where-Object { $_.Name -ne '.gitkeep' }
            $sttMB = ($items | ForEach-Object { Get-DirSizeMB -Path $_.FullName } | Measure-Object -Sum).Sum
            if (-not $sttMB) { $sttMB = 0 }
            if ($items.Count -eq 0) {
                Write-Host "  - src-tauri/binaries/lashon-stt/*                        (clean)" -ForegroundColor DarkGray
            }
            elseif ($DryRun) {
                Write-Host ("  - src-tauri/binaries/lashon-stt/* (keep .gitkeep)        {0,8} MB  (dry-run)" -f $sttMB) -ForegroundColor Yellow
                $reclaimed += $sttMB
            }
            else {
                $items | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
                Write-Host ("  - src-tauri/binaries/lashon-stt/* (keep .gitkeep)        {0,8} MB  deleted" -f $sttMB) -ForegroundColor Green
                $reclaimed += $sttMB
            }
        }

        # llama-server bundle — keep .gitkeep + README.md, blow away the DLLs/exe.
        $llamaBin = Join-Path $root 'apps\desktop\src-tauri\binaries\llama-server'
        if (Test-Path $llamaBin) {
            $items = Get-ChildItem -LiteralPath $llamaBin -Force | Where-Object { $_.Name -notin @('.gitkeep', 'README.md') }
            $llamaMB = ($items | ForEach-Object { Get-DirSizeMB -Path $_.FullName } | Measure-Object -Sum).Sum
            if (-not $llamaMB) { $llamaMB = 0 }
            if ($items.Count -eq 0) {
                Write-Host "  - src-tauri/binaries/llama-server/* (keep .gitkeep+README)  (clean)" -ForegroundColor DarkGray
            }
            elseif ($DryRun) {
                Write-Host ("  - src-tauri/binaries/llama-server/* (keep .gitkeep+README)  {0,8} MB  (dry-run)" -f $llamaMB) -ForegroundColor Yellow
                $reclaimed += $llamaMB
            }
            else {
                $items | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
                Write-Host ("  - src-tauri/binaries/llama-server/* (keep .gitkeep+README)  {0,8} MB  deleted" -f $llamaMB) -ForegroundColor Green
                $reclaimed += $llamaMB
            }
        }
    }

    if ($Level -eq 'aggressive') {
        Write-Host ""
        Write-Host "Downloaded model weights (re-downloaded on next run):" -ForegroundColor Cyan
        $reclaimed += Remove-PathSafely -Path (Join-Path $root 'models\stt')       -Label 'models/stt'       -DryRun:$DryRun
        $reclaimed += Remove-PathSafely -Path (Join-Path $root 'models\local-llm') -Label 'models/local-llm' -DryRun:$DryRun
    }

    Write-Host ""
    if ($DryRun) {
        Write-Host ("Projected reclaim: {0} MB ({1} GB)" -f $reclaimed, [math]::Round($reclaimed / 1024, 2)) -ForegroundColor Cyan
        Write-Host "Re-run without -DryRun to apply." -ForegroundColor Cyan
    }
    else {
        $afterMB = Get-RepoTotalMB -Root $root
        $afterGB = [math]::Round($afterMB / 1024, 2)
        $deltaMB = $beforeMB - $afterMB
        $deltaGB = [math]::Round($deltaMB / 1024, 2)
        Write-Host ("Repo total after:  {0} GB" -f $afterGB) -ForegroundColor Cyan
        Write-Host ("Reclaimed:         {0} GB ({1} MB)" -f $deltaGB, $deltaMB) -ForegroundColor Cyan
    }
}
finally {
    Pop-Location
}
