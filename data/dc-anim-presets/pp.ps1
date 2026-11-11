$ErrorActionPreference = 'Stop'
$app = New-Object -ComObject PowerPoint.Application
$app.DisplayAlerts = 1
$presentation = $null
$capturePath = Join-Path $env:TEMP "deckcraft-animation-presets-$PID.pptx"
$records = New-Object System.Collections.Generic.List[object]
# Output columns: class, enum name and value, saved PPTX preset id and subtype, COM duration in ms.
# Each effect gets a separate synthetic slide so PPTX preset metadata cannot be mismatched by order.
function duration_ms($timing) {
    try {
        $seconds = [double]$timing.Duration
        if ([double]::IsNaN($seconds) -or [double]::IsInfinity($seconds)) { return 'unknown' }
        return [int][Math]::Round($seconds * 1000)
    } catch {
        return 'unknown'
    }
}
try {
    $assembly = [Reflection.Assembly]::LoadWithPartialName('Microsoft.Office.Interop.PowerPoint')
    if ($null -eq $assembly) { throw 'PowerPoint interop assembly was not available.' }
    $enumType = $assembly.GetType('Microsoft.Office.Interop.PowerPoint.MsoAnimEffect')
    if ($null -eq $enumType) { throw 'MsoAnimEffect was not available in the interop assembly.' }

    $presentation = $app.Presentations.Add()
    foreach ($value in [Enum]::GetValues($enumType)) {
        $name = [Enum]::GetName($enumType, $value)
        try {
            $slide = $presentation.Slides.Add($presentation.Slides.Count + 1, 12)
            $shape = $slide.Shapes.AddShape(1, 100, 100, 100, 100)
            $shape.TextFrame.TextRange.Text = 'Sample text'
            $effect = $slide.TimeLine.MainSequence.AddEffect($shape, [int]$value, 0, 1)
            $class = if ([int]$value -ge 54 -and [int]$value -le 82) { 'emph' } elseif ([int]$value -ge 86 -and [int]$value -le 149) { 'path' } else { 'entr' }
            $records.Add([pscustomobject]@{ Name = $name; Type = [int]$value; Class = $class; DurationMs = (duration_ms $effect.Timing); Slide = $slide.SlideIndex })
            if ([int]$value -gt 0 -and [int]$value -lt 54) {
                $exitSlide = $presentation.Slides.Add($presentation.Slides.Count + 1, 12)
                $exitShape = $exitSlide.Shapes.AddShape(1, 100, 100, 100, 100)
                $exitShape.TextFrame.TextRange.Text = 'Sample text'
                $exit = $exitSlide.TimeLine.MainSequence.AddEffect($exitShape, [int]$value, 0, 1)
                try {
                    $exit.Exit = -1
                    $records.Add([pscustomobject]@{ Name = $name; Type = [int]$value; Class = 'exit'; DurationMs = (duration_ms $exit.Timing); Slide = $exitSlide.SlideIndex })
                } catch {
                    $exitSlide.Delete()
                    throw
                }
            }
        } catch {
            (@($name, [int]$value, 'ERROR', $_.Exception.Message) -join [char]9)
        }
    }

    $presentation.SaveAs($capturePath, 24)
    [Reflection.Assembly]::LoadWithPartialName('System.IO.Compression.FileSystem') | Out-Null
    $zip = [IO.Compression.ZipFile]::OpenRead($capturePath)
    try {
        foreach ($record in $records) {
            $entry = $zip.GetEntry("ppt/slides/slide$($record.Slide).xml")
            $reader = New-Object IO.StreamReader($entry.Open())
            try { [xml]$xml = $reader.ReadToEnd() } finally { $reader.Dispose() }
            $ns = New-Object Xml.XmlNamespaceManager($xml.NameTable)
            $ns.AddNamespace('p', 'http://schemas.openxmlformats.org/presentationml/2006/main')
            $timing = $xml.SelectSingleNode('//p:cTn[@presetID]', $ns)
            $durationMs = 0
            foreach ($node in $timing.SelectNodes('.//p:cTn[@dur]', $ns)) {
                $duration = 0
                if ([uint32]::TryParse($node.GetAttribute('dur'), [ref]$duration) -and $duration -gt $durationMs) { $durationMs = $duration }
            }
            (@($record.Class, $record.Name, $record.Type, $timing.GetAttribute('presetID'), $timing.GetAttribute('presetSubtype'), $record.DurationMs) -join [char]9)
        }
    } finally { $zip.Dispose() }
} finally {
    if ($null -ne $presentation) { $presentation.Close() }
    $app.Quit()
    if (Test-Path $capturePath) { Remove-Item -LiteralPath $capturePath -Force }
}
