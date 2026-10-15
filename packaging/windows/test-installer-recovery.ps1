[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$RecoveryScript,
    [Parameter(Mandatory = $true)][string]$WorkDir,
    [switch]$SourceOnly,
    [switch]$CacheOnly
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$installer = New-Object -ComObject WindowsInstaller.Installer
$run = [Guid]::NewGuid().ToString()
$root = Join-Path $WorkDir $run
New-Item -ItemType Directory -Path $root | Out-Null
$upgrade = '{' + [Guid]::NewGuid().ToString().ToUpperInvariant() + '}'
$component = '{' + [Guid]::NewGuid().ToString().ToUpperInvariant() + '}'

function Sql($db, [string]$sql) {
    $view = $db.OpenView($sql)
    [void]$view.Execute()
    [void]$view.Close()
}

function New-Fixture([string]$path, [string]$version) {
    $product = '{' + [Guid]::NewGuid().ToString().ToUpperInvariant() + '}'
    $package = '{' + [Guid]::NewGuid().ToString().ToUpperInvariant() + '}'
    $db = $installer.OpenDatabase($path, 3)
    Sql $db 'CREATE TABLE `Media` (`DiskId` SHORT NOT NULL, `LastSequence` LONG NOT NULL, `DiskPrompt` CHAR(64) LOCALIZABLE, `Cabinet` CHAR(255), `VolumeLabel` CHAR(32), `Source` CHAR(72) PRIMARY KEY `DiskId`)'
    Sql $db 'INSERT INTO `Media` (`DiskId`, `LastSequence`) VALUES (1, 0)'
    Sql $db 'CREATE TABLE `Property` (`Property` CHAR(72) NOT NULL, `Value` CHAR(0) LOCALIZABLE PRIMARY KEY `Property`)'
    foreach ($entry in @{
        ProductCode=$product; ProductVersion=$version; ProductName='Synthetic Installer Recovery';
        Manufacturer='Test Fixture'; ProductLanguage='1033'; UpgradeCode=$upgrade;
        ALLUSERS='1'; SecureCustomProperties='OLDPRODUCTS'
    }.GetEnumerator()) { Sql $db "INSERT INTO ``Property`` (``Property``, ``Value``) VALUES ('$($entry.Key)', '$($entry.Value)')" }
    Sql $db 'CREATE TABLE `Directory` (`Directory` CHAR(72) NOT NULL, `Directory_Parent` CHAR(72), `DefaultDir` CHAR(255) NOT NULL LOCALIZABLE PRIMARY KEY `Directory`)'
    Sql $db "INSERT INTO ``Directory`` (``Directory``, ``DefaultDir``) VALUES ('TARGETDIR', 'SourceDir')"
    Sql $db 'CREATE TABLE `Component` (`Component` CHAR(72) NOT NULL, `ComponentId` CHAR(38), `Directory_` CHAR(72) NOT NULL, `Attributes` SHORT NOT NULL, `Condition` CHAR(255), `KeyPath` CHAR(72) PRIMARY KEY `Component`)'
    Sql $db "INSERT INTO ``Component`` (``Component``, ``ComponentId``, ``Directory_``, ``Attributes``, ``KeyPath``) VALUES ('Fixture', '$component', 'TARGETDIR', 260, 'FixtureValue')"
    Sql $db 'CREATE TABLE `Feature` (`Feature` CHAR(38) NOT NULL, `Feature_Parent` CHAR(38), `Title` CHAR(64) LOCALIZABLE, `Description` CHAR(255) LOCALIZABLE, `Display` SHORT, `Level` SHORT NOT NULL, `Directory_` CHAR(72), `Attributes` SHORT NOT NULL PRIMARY KEY `Feature`)'
    Sql $db "INSERT INTO ``Feature`` (``Feature``, ``Level``, ``Attributes``) VALUES ('Main', 1, 0)"
    Sql $db 'CREATE TABLE `FeatureComponents` (`Feature_` CHAR(38) NOT NULL, `Component_` CHAR(72) NOT NULL PRIMARY KEY `Feature_`, `Component_`)'
    Sql $db "INSERT INTO ``FeatureComponents`` (``Feature_``, ``Component_``) VALUES ('Main', 'Fixture')"
    Sql $db 'CREATE TABLE `Registry` (`Registry` CHAR(72) NOT NULL, `Root` SHORT NOT NULL, `Key` CHAR(255) NOT NULL LOCALIZABLE, `Name` CHAR(255) LOCALIZABLE, `Value` CHAR(0) LOCALIZABLE, `Component_` CHAR(72) NOT NULL PRIMARY KEY `Registry`)'
    Sql $db "INSERT INTO ``Registry`` (``Registry``, ``Root``, ``Key``, ``Name``, ``Value``, ``Component_``) VALUES ('FixtureValue', 2, 'Software\ScryerInstallerFixture\$run', 'Version', '[ProductVersion]', 'Fixture')"
    Sql $db 'CREATE TABLE `Upgrade` (`UpgradeCode` CHAR(38) NOT NULL, `VersionMin` CHAR(20), `VersionMax` CHAR(20), `Language` CHAR(255), `Attributes` LONG NOT NULL, `Remove` CHAR(255), `ActionProperty` CHAR(72) NOT NULL PRIMARY KEY `UpgradeCode`, `VersionMin`, `VersionMax`, `Language`, `Attributes`)'
    Sql $db "INSERT INTO ``Upgrade`` (``UpgradeCode``, ``VersionMax``, ``Attributes``, ``Remove``, ``ActionProperty``) VALUES ('$upgrade', '$version', 1, 'ALL', 'OLDPRODUCTS')"
    Sql $db 'CREATE TABLE `InstallExecuteSequence` (`Action` CHAR(72) NOT NULL, `Condition` CHAR(255), `Sequence` SHORT PRIMARY KEY `Action`)'
    foreach ($action in @{
        FindRelatedProducts=25; ValidateProductID=700; CostInitialize=800; FileCost=900; CostFinalize=1000;
        InstallValidate=1400; RemoveExistingProducts=1450; InstallInitialize=1500; ProcessComponents=1600;
        UnpublishFeatures=1800; RemoveRegistryValues=2600; WriteRegistryValues=5000;
        RegisterUser=6000; RegisterProduct=6100; PublishFeatures=6300; PublishProduct=6400; InstallFinalize=6600
    }.GetEnumerator()) { Sql $db "INSERT INTO ``InstallExecuteSequence`` (``Action``, ``Sequence``) VALUES ('$($action.Key)', $($action.Value))" }
    $summary = $db.SummaryInformation(20)
    $summary.Property(2) = 'Installation Database'
    $summary.Property(7) = 'x64;1033'
    $summary.Property(9) = $package
    $summary.Property(14) = 200
    $summary.Property(15) = 2
    $summary.Persist()
    $db.Commit()
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($summary)
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($db)
    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
    return @{ product_code=$product; package_code=$package; version=$version }
}

function Invoke-Msi([string]$operation, [string]$target, [string]$log) {
    $process = Start-Process "$env:SystemRoot\System32\msiexec.exe" -ArgumentList @($operation, ('"'+$target+'"'), '/qn', '/norestart', '/l*v', ('"'+$log+'"')) -PassThru
    if (-not $process.WaitForExit(120000)) { throw "Fixture MSI timed out: $log" }
    return $process.ExitCode
}

# Keep all fixture artifacts for diagnosis. Only these generated MSI product
# identities may be installed/uninstalled; no Scryer installation is touched.
$oldPath = Join-Path $root 'artifact'
$newPath = Join-Path $root 'new.msi'
$old = New-Fixture $oldPath '1.0.0'
$new = New-Fixture $newPath '1.0.1'
$cache = Join-Path $root 'recovery-cache'
New-Item -ItemType Directory -Path $cache | Out-Null
$retained = Join-Path $cache 'installer.msi'
Copy-Item -LiteralPath $oldPath -Destination $retained
$sentinel = Join-Path $root 'user-data.txt'
Set-Content -LiteralPath $sentinel -Value 'preserve me'
try {
    if ((Invoke-Msi '/i' $oldPath (Join-Path $root 'install.log')) -ne 0) { throw 'Fixture install failed' }
    $cached = $installer.ProductInfo($old.product_code, 'LocalPackage')
    if (-not $CacheOnly) {
        Move-Item -LiteralPath $oldPath -Destination "$oldPath.unavailable"
    }
    if (-not $SourceOnly) {
        Move-Item -LiteralPath $cached -Destination (Join-Path $root 'cached-unavailable.msi')
    }
    $failure = Invoke-Msi '/i' $newPath (Join-Path $root 'missing-source.log')
    if ($SourceOnly -or $CacheOnly) {
        if ($failure -ne 0) { throw 'Upgrade with a retained source or intact installer cache failed' }
        [ordered]@{ passed=$true; source_missing=[bool]$SourceOnly; cache_missing=[bool]$CacheOnly; evidence=$root } | ConvertTo-Json -Compress
        return
    }
    if ($failure -eq 0) { throw 'Missing source/cache did not reproduce a failed major upgrade' }

    $env:SCRYER_MSI_OPERATION = 'recache'
    $env:SCRYER_MSI_PACKAGE = $newPath
    $env:SCRYER_MSI_PRODUCT_CODE = $old.product_code
    $env:SCRYER_MSI_PACKAGE_CODE = $old.package_code
    $env:SCRYER_MSI_VERSION = $old.version
    $rejected = $false
    try { & $RecoveryScript | Out-Null } catch { $rejected = $true }
    if (-not $rejected) { throw 'Wrong installer was accepted' }
    $env:SCRYER_MSI_PACKAGE = $retained
    & $RecoveryScript | Out-Null
    & $RecoveryScript | Out-Null
    if (Test-Path -LiteralPath $oldPath) { throw 'Recovery wrote to the old source directory' }
    if (-not (Test-Path -LiteralPath $installer.ProductInfo($old.product_code, 'LocalPackage'))) { throw 'Cache not recovered' }
    $env:SCRYER_MSI_OPERATION = 'install'
    $env:SCRYER_MSI_PACKAGE = $newPath
    & $RecoveryScript | Out-Null
    if ($installer.ProductInfo($new.product_code, 'VersionString') -ne '1.0.1') { throw 'New version not installed' }
    if ((Get-Content -LiteralPath $sentinel -Raw).Trim() -ne 'preserve me') { throw 'User data changed' }
    [ordered]@{ passed=$true; original_failure=$failure; evidence=$root; scenarios=@('missing source and cache', 'wrong package rejected', 'recache', 'major upgrade after recovery', 'user data preserved') } | ConvertTo-Json -Compress
} finally {
    foreach ($product in @($new.product_code, $old.product_code)) {
        if ($installer.ProductState($product) -ne -1) {
            $result = Invoke-Msi '/x' $product (Join-Path $root ($product + '.uninstall.log'))
            if ($result -notin @(0,1605)) { Write-Warning "Synthetic product cleanup failed: $product ($result)" }
        }
    }
}
