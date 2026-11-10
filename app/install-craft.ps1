param([string]$Destination = (Join-Path $PSScriptRoot 'craft-apps'))

$ErrorActionPreference = 'Stop'
$releases = @(
    @{ Name = 'wordcraft'; Version = '0.16.0'; Sha256 = '612FCBE9C4FD8C8A2538E4B711DD78EB448859BBDB7F0F4517B26E99511D6F0E' },
    @{ Name = 'deckcraft'; Version = '0.16.0'; Sha256 = '8795D58AB02C30AD25F38525A79EE0E69C40B3D122E223DC04D2703827621F55' },
    @{ Name = 'gridcraft'; Version = '0.17.0'; Sha256 = '6FCD15C008B80945BC1A1D04757BAED0800C69CC58525E62B9820C67B791176B' },
    @{ Name = 'pdfcraft'; Version = '0.19.0'; Sha256 = '6CECEA09DB4D010F1AE18D4C3170DB243C7ADC653699BA2711F570D57FED6947' }
)

$root = (New-Item -ItemType Directory -Path $Destination -Force).FullName
$cache = (New-Item -ItemType Directory -Path (Join-Path $PSScriptRoot 'target/craft-archives') -Force).FullName
foreach ($release in $releases) {
    $name = $release.Name
    $version = $release.Version
    $folder = Join-Path $root $name
    if (Test-Path -LiteralPath (Join-Path $folder 'index.html') -PathType Leaf) {
        Write-Host "$name ya está instalado."
        continue
    }
    if (Test-Path -LiteralPath $folder) { throw "La carpeta $folder existe sin index.html." }

    $package = "$name-web-$version"
    $archive = Join-Path $cache "$name.zip"
    if (!(Test-Path -LiteralPath $archive) -or (Get-FileHash -LiteralPath $archive).Hash -ne $release.Sha256) {
        Invoke-WebRequest "https://github.com/storytold/$name/releases/download/v$version/$package.zip" -OutFile $archive
    }
    if ((Get-FileHash -LiteralPath $archive).Hash -ne $release.Sha256) {
        throw "La suma SHA-256 de $package.zip no coincide."
    }

    Expand-Archive -LiteralPath $archive -DestinationPath $root -Force
    $unpacked = Join-Path $root $package
    if (!(Test-Path -LiteralPath (Join-Path $unpacked 'index.html') -PathType Leaf)) {
        throw "Falta index.html en $package."
    }
    Rename-Item -LiteralPath $unpacked -NewName $name
    Write-Host "$name $version instalado."
}
