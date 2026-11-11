param(
    [string]$Destination = (Join-Path $PSScriptRoot 'craft-apps/wordcraft'),
    [string]$Bindgen = 'wasm-bindgen'
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$source = Join-Path $PSScriptRoot 'target/wordcraft-live-source'
$patch = Join-Path $PSScriptRoot 'wordcraft-live/bridge.patch'
$revision = '02a802cbafe45b5c8fecc00490c8f6b78c5ba251'
if ((& $Bindgen --version) -ne 'wasm-bindgen 0.2.129') {
    throw 'Se necesita wasm-bindgen-cli 0.2.129.'
}
if (!(Test-Path (Join-Path $source '.git'))) {
    git clone --depth 1 --branch v0.16.0 https://github.com/storytold/wordcraft.git $source
}
if ((git -C $source rev-parse HEAD) -ne $revision) { throw "Se esperaba WordCraft $revision." }
if (!(git -C $source diff --name-only -- apps/wordcraft-web/src/main.rs apps/wordcraft-web/src/web.rs)) {
    git -C $source apply --unidiff-zero $patch
} else {
    git -C $source apply --unidiff-zero --reverse --check $patch
}
Copy-Item (Join-Path $PSScriptRoot 'wordcraft-live/live.rs') (Join-Path $source 'apps/wordcraft-web/src/live.rs')

$catalog = Join-Path $source 'craft-i18n'
$catalogRev = (Get-Content (Join-Path $source 'craft-i18n.rev') -Raw).Trim()
if (!(Test-Path (Join-Path $catalog '.git'))) {
    git init $catalog
    git -C $catalog fetch --depth 1 https://github.com/storytold/craft-i18n.git $catalogRev
    git -C $catalog checkout --detach FETCH_HEAD
}
if ((git -C $catalog rev-parse HEAD) -ne $catalogRev) { throw 'La revisión de craft-i18n no coincide.' }

$env:CRAFT_I18N_DIR = $catalog
$env:CARGO_NET_GIT_FETCH_WITH_CLI = 'true'
rustup target add wasm32-unknown-unknown
cargo build --manifest-path (Join-Path $source 'Cargo.toml') --locked --release --target wasm32-unknown-unknown -p wordcraft-web

$wasm = Join-Path $source 'target/wasm32-unknown-unknown/release/wordcraft-web.wasm'
$version = (Get-FileHash -LiteralPath $wasm).Hash.Substring(0, 12).ToLowerInvariant()
$name = "wordcraft-live-$version"
$out = (New-Item -ItemType Directory -Path $Destination -Force).FullName
& $Bindgen --target web --no-typescript --out-dir $out --out-name $name $wasm
Copy-Item (Join-Path $source 'assets/app-icon/wordcraft.ico') $out
$html = Get-Content (Join-Path $source 'apps/wordcraft-web/index.html') -Raw
$html = $html -replace '<link data-trunk rel="icon"[^>]+>', '<link rel="icon" href="./wordcraft.ico">'
$html = $html -replace '<link data-trunk rel="rust"[^>]+>', "<script type='module'>import init from './$name.js'; await init();</script>"
$html | Set-Content (Join-Path $out 'index.html')
Write-Host "WordCraft 0.16.0 con edición en directo instalado en $out."
