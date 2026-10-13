$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

$repoRoot = Split-Path -Parent $PSScriptRoot
$iconsDirectory = Join-Path $repoRoot "icons"
$sourcePath = Join-Path $iconsDirectory "usage-source.png"
$previewPath = Join-Path $iconsDirectory "usage.png"
$iconPath = Join-Path $iconsDirectory "usage.ico"
$sizes = @(16, 24, 32, 48, 64, 128, 256)
$sourceImage = [System.Drawing.Image]::FromFile($sourcePath)

if ($sourceImage.Width -ne $sourceImage.Height) {
  $sourceImage.Dispose()
  throw "The app icon source must be square."
}

$entries = [System.Collections.Generic.List[object]]::new()

try {
  foreach ($size in $sizes) {
    $bitmap = [System.Drawing.Bitmap]::new(
      $size,
      $size,
      [System.Drawing.Imaging.PixelFormat]::Format32bppArgb
    )
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.Clear([System.Drawing.Color]::Transparent)
    $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceOver
    $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $graphics.DrawImage($sourceImage, 0, 0, $size, $size)

    $pngStream = [System.IO.MemoryStream]::new()
    $bitmap.Save($pngStream, [System.Drawing.Imaging.ImageFormat]::Png)
    $pngBytes = $pngStream.ToArray()
    if ($size -eq 256) {
      [System.IO.File]::WriteAllBytes($previewPath, $pngBytes)
    }
    $entries.Add(@{ Size = $size; PngBytes = $pngBytes })

    $pngStream.Dispose()
    $graphics.Dispose()
    $bitmap.Dispose()
  }
}
finally {
  $sourceImage.Dispose()
}

$iconStream = [System.IO.MemoryStream]::new()
$writer = [System.IO.BinaryWriter]::new($iconStream)
$writer.Write([uint16]0)
$writer.Write([uint16]1)
$writer.Write([uint16]$entries.Count)
$imageOffset = 6 + (16 * $entries.Count)

foreach ($entry in $entries) {
  $dimension = if ($entry.Size -eq 256) { 0 } else { $entry.Size }
  $writer.Write([byte]$dimension)
  $writer.Write([byte]$dimension)
  $writer.Write([byte]0)
  $writer.Write([byte]0)
  $writer.Write([uint16]1)
  $writer.Write([uint16]32)
  $writer.Write([uint32]$entry.PngBytes.Length)
  $writer.Write([uint32]$imageOffset)
  $imageOffset += $entry.PngBytes.Length
}

foreach ($entry in $entries) {
  $writer.Write($entry.PngBytes)
}

$writer.Flush()
[System.IO.File]::WriteAllBytes($iconPath, $iconStream.ToArray())
$writer.Dispose()
$iconStream.Dispose()

Write-Output "Generated usage.png and multi-size usage.ico from usage-source.png."
