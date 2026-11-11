param (
    [string]$DictPath = "locales\ko.json",
    [string]$TargetDir = "crates\ui-egui\src"
)

Set-Location "C:\dev\deckcraft"

if (-not (Test-Path $DictPath)) {
    Write-Error "사전 파일이 없습니다: $DictPath"
    exit 1
}

$Dict = Get-Content $DictPath -Raw -Encoding UTF8 | ConvertFrom-Json
$Files = Get-ChildItem -Path $TargetDir -Recurse -Filter "*.rs"
Write-Host ">>> 자동 번역 주입 시작 ($($Files.Count)개 파일 검사)" -ForegroundColor Cyan
$ModifiedCount = 0

foreach ($File in $Files) {
    $Content = [System.IO.File]::ReadAllText($File.FullName, [System.Text.Encoding]::UTF8)
    $Changed = $false
    
    foreach ($Prop in $Dict.PSObject.Properties) {
        $EscapedKey = [regex]::Escape($Prop.Name)
        # 사전 키에 '...'가 있으면 소스코드의 '...'와 유니코드 '…' 모두 매칭
        $FlexibleKey = $EscapedKey -replace '\\\.\\\.\\\.', '(?:\.\.\.|…)'
        $SearchPattern = "`"$FlexibleKey`""
        $ReplacePattern = "`"$($Prop.Value)`""
        
        if ($Content -match $SearchPattern) {
            $Content = $Content -creplace $SearchPattern, $ReplacePattern
            $Changed = $true
        }
    }
    
    if ($Changed) {
        [System.IO.File]::WriteAllText($File.FullName, $Content, [System.Text.Encoding]::UTF8)
        Write-Host "자동 주입 완료: $($File.Name)" -ForegroundColor Green
        $ModifiedCount++
    }
}

Write-Host ">>> 총 $($ModifiedCount)개 파일 갱신됨. 무결성 검증 중..." -ForegroundColor Cyan
cargo check -p deckcraft-ui-egui

if ($LASTEXITCODE -eq 0) {
    git diff > C:\dev\deckcraft_korean.patch
    Write-Host ">>> 패치 파일 동기화 성공 (C:\dev\deckcraft_korean.patch)" -ForegroundColor Green
} else {
    Write-Error ">>> 빌드 검증 실패! git restore 또는 수동 롤백이 필요합니다."
}