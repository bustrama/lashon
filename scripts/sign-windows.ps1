#requires -Version 5.1
<#
.SYNOPSIS
    Authenticode-sign Ottid's Windows binaries with Azure Artifact Signing.

.DESCRIPTION
    The one signer of a release build (docs/adr/0043). Three modes:

      sign-windows.ps1 <file> [<file>...]
          Sign these files. Tauri's bundle.windows.signCommand
          (src-tauri/tauri.signing.conf.json) calls it this way for the main
          exe, the NSIS plugins, the installer and (from inside makensis) the
          uninstaller; release.yml for the portable zip's exe.

      sign-windows.ps1 -Tree <dir> [<dir>...]
          Sign every PE image under these directories that does not already
          carry a valid signature. release.yml runs it on the staged bundle
          resources before `tauri build`: the frozen ottid-stt sidecar and, in
          the full edition, llama-server. Tauri signs resources too, but only
          *.exe / *.dll, one process per file; the sidecar also ships ~90 *.pyd
          modules, and Smart App Control checks every module the loader maps,
          whatever its extension.

      sign-windows.ps1 -Verify <file-or-dir> [...]
          Sign nothing; fail unless every PE image given, or under a directory
          given, carries a valid signature.

    Files go through signtool with the Artifact Signing dlib: a SHA-256 digest
    and an RFC 3161 timestamp. The timestamp is not optional: Artifact Signing
    certificates live three days, and only the timestamp keeps a signature valid
    after that. The dlib authenticates through the Azure CLI's session only:
    every other DefaultAzureCredential source is excluded, so it never guesses.
    In CI that session is the one azure/login opened over GitHub OIDC, so no
    signing secret exists anywhere; on a workstation it is `az login`.

    Environment for signing (release.yml sets all of it):
      OTTID_SIGNTOOL              signtool.exe
      OTTID_SIGNING_DLIB          Azure.CodeSigning.Dlib.dll
      ARTIFACT_SIGNING_ENDPOINT   e.g. https://weu.codesigning.azure.net
      ARTIFACT_SIGNING_ACCOUNT    the Artifact Signing account name
      ARTIFACT_SIGNING_PROFILE    the certificate profile name

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/sign-windows.ps1 target/release/ottid.exe

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/sign-windows.ps1 -Tree apps/desktop/src-tauri/binaries

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/sign-windows.ps1 -Verify C:\ottid-check
#>
[CmdletBinding(DefaultParameterSetName = 'Files')]
param(
    [Parameter(Mandatory, Position = 0, ValueFromRemainingArguments)]
    [string[]] $Path,

    [Parameter(ParameterSetName = 'Tree')]
    [switch] $Tree,

    [Parameter(ParameterSetName = 'Verify')]
    [switch] $Verify
)

$ErrorActionPreference = 'Stop'

# RFC 3161 timestamp authority for Artifact Signing certificates.
$TimestampUrl = 'http://timestamp.acs.microsoft.com'
# signtool takes the files on one command line; batches keep it well under
# the 32K-character limit.
$BatchSize = 50
# Signing and timestamping are network calls; a transient failure gets two
# more tries before it fails the build. Re-signing a file replaces its
# signature, so a retried batch is safe.
$Attempts = 3

function Get-RequiredEnv([string] $Name) {
    $value = [Environment]::GetEnvironmentVariable($Name)
    if ([string]::IsNullOrWhiteSpace($value)) {
        throw "sign-windows: $Name is not set (see docs/adr/0043)."
    }
    $value
}

# True for a PE image: the 'MZ' DOS header, then 'PE\0\0' at e_lfanew.
function Test-PortableExecutable([string] $File) {
    $stream = [IO.File]::OpenRead($File)
    try {
        if ($stream.Length -lt 64) { return $false }
        $reader = [IO.BinaryReader]::new($stream)
        if ($reader.ReadUInt16() -ne 0x5A4D) { return $false }
        $stream.Position = 0x3C
        $peOffset = $reader.ReadInt32()
        if ($peOffset -lt 0 -or $peOffset + 4 -gt $stream.Length) { return $false }
        $stream.Position = $peOffset
        return $reader.ReadUInt32() -eq 0x00004550
    }
    finally {
        $stream.Dispose()
    }
}

# The PE images among the given files and under the given directories.
function Get-PortableExecutable([string[]] $Paths) {
    foreach ($p in $Paths) {
        $item = Get-Item -LiteralPath $p
        $candidates = if ($item.PSIsContainer) { Get-ChildItem -LiteralPath $p -Recurse -File } else { $item }
        $candidates | Where-Object { Test-PortableExecutable $_.FullName } | ForEach-Object FullName
    }
}

function Test-ValidSignature([string] $File) {
    (Get-AuthenticodeSignature -LiteralPath $File).Status -eq 'Valid'
}

if ($Verify) {
    $images = @(Get-PortableExecutable $Path)
    $unsigned = @($images | Where-Object { -not (Test-ValidSignature $_) })
    Write-Host "sign-windows: $($images.Count - $unsigned.Count) of $($images.Count) PE image(s) validly signed"
    if ($unsigned.Count -gt 0) {
        $unsigned | ForEach-Object {
            Write-Host "  $((Get-AuthenticodeSignature -LiteralPath $_).Status): $_"
        }
        throw "sign-windows: $($unsigned.Count) PE image(s) without a valid signature"
    }
    return
}

if ($Tree) {
    $files = @(Get-PortableExecutable $Path | Where-Object { -not (Test-ValidSignature $_) })
    Write-Host "sign-windows: $($files.Count) unsigned PE image(s) under $($Path -join ', ')"
}
else {
    $files = @($Path | ForEach-Object { (Resolve-Path -LiteralPath $_).ProviderPath })
}
if ($files.Count -eq 0) { return }

$signtool = Get-RequiredEnv 'OTTID_SIGNTOOL'
$dlib = Get-RequiredEnv 'OTTID_SIGNING_DLIB'
$metadata = [ordered]@{
    Endpoint               = Get-RequiredEnv 'ARTIFACT_SIGNING_ENDPOINT'
    CodeSigningAccountName = Get-RequiredEnv 'ARTIFACT_SIGNING_ACCOUNT'
    CertificateProfileName = Get-RequiredEnv 'ARTIFACT_SIGNING_PROFILE'
    ExcludeCredentials     = @(
        'EnvironmentCredential'
        'WorkloadIdentityCredential'
        'ManagedIdentityCredential'
        'SharedTokenCacheCredential'
        'VisualStudioCredential'
        'VisualStudioCodeCredential'
        'AzurePowerShellCredential'
        'AzureDeveloperCliCredential'
        'InteractiveBrowserCredential'
    )
}

$metadataFile = Join-Path ([IO.Path]::GetTempPath()) "ottid-signing-$PID.json"
[IO.File]::WriteAllText($metadataFile, ($metadata | ConvertTo-Json))
try {
    for ($i = 0; $i -lt $files.Count; $i += $BatchSize) {
        $batch = $files[$i..([Math]::Min($i + $BatchSize, $files.Count) - 1)]
        for ($attempt = 1; ; $attempt++) {
            & $signtool sign /v /fd SHA256 /tr $TimestampUrl /td SHA256 `
                /d Ottid /du https://github.com/bustrama/ottid `
                /dlib $dlib /dmdf $metadataFile @batch
            if ($LASTEXITCODE -eq 0) { break }
            if ($attempt -eq $Attempts) {
                throw "sign-windows: signtool exited with $LASTEXITCODE after $Attempts attempts"
            }
            Write-Warning "sign-windows: signtool exited with $LASTEXITCODE; retrying ($attempt/$Attempts)"
            Start-Sleep -Seconds (10 * $attempt)
        }
    }
}
finally {
    Remove-Item -LiteralPath $metadataFile -ErrorAction SilentlyContinue
}
