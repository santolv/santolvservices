# Builds the ArtCraft editors' web bundles from the releases pinned in backend/office-editors.toml
# and installs them into backend/servidor/apps/<editor>/ (served at /apps/<editor>/).
# Requires: Rust with the wasm32-unknown-unknown target and wasm-bindgen-cli matching each
# editor's locked wasm-bindgen version. Every build uses the project's own Cargo.lock (--locked).
param(
  [string]$Source = (Join-Path $PSScriptRoot '..\..\artcraft-src'),
  [string]$Install = (Join-Path $PSScriptRoot '..\backend\servidor\apps'),
  [string[]]$Only
)
$ErrorActionPreference = 'Stop'
$manifest = Get-Content (Join-Path $PSScriptRoot '..\backend\office-editors.toml') -Raw
$editors = [regex]::Matches($manifest, '(?ms)^\[(\w+)\]\s*\r?\nrepository = "([^"]+)"\s*\r?\ntag = "([^"]+)"\s*\r?\ncommit = "([0-9a-f]{40})"\s*\r?\npackage = "([^"]+)"')
New-Item -ItemType Directory -Force $Source, $Install | Out-Null
foreach ($e in $editors) {
  $name, $repo, $tag, $commit, $package = $e.Groups[1..5].Value
  if ($Only -and $name -notin $Only) { continue }
  $dir = Join-Path $Source $name
  if (-not (Test-Path $dir)) { git clone --quiet --depth 1 --branch $tag $repo $dir }
  $head = (git -C $dir rev-parse HEAD).Trim()
  if ($head -ne $commit) { throw "$name: $tag is $head, expected $commit. Refusing to build." }
  Push-Location $dir
  try { cargo build --locked --release --target wasm32-unknown-unknown -p $package } finally { Pop-Location }
  if ($LASTEXITCODE) { throw "$name: build failed" }
  $wasm = Join-Path $dir "target\wasm32-unknown-unknown\release\$package.wasm"
  $out = Join-Path $Install $name
  $staging = "$out.new"
  if (Test-Path $staging) { Remove-Item -Recurse -Force $staging }
  wasm-bindgen --target web --no-typescript --out-dir $staging $wasm
  if ($LASTEXITCODE) { throw "$name: wasm-bindgen failed" }
  Set-Content (Join-Path $staging 'RELEASE.txt') "$repo $tag $commit" -Encoding utf8
  if (Test-Path $out) { Remove-Item -Recurse -Force $out }
  Move-Item $staging $out
  Write-Output "$name $tag ($commit) -> $out"
}
