$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$ProgressPreference = 'SilentlyContinue'
$installer = New-Object -ComObject WindowsInstaller.Installer

function Read-Property($database, [string]$name) {
    $view = $database.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='$name'")
    [void]$view.Execute()
    $record = $view.Fetch()
    if (-not $record) { throw "Installer property missing: $name" }
    $value = $record.StringData(1)
    [void]$view.Close()
    return $value
}

switch ($env:SCRYER_MSI_OPERATION) {
    'discover' {
        $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
        $principal = New-Object Security.Principal.WindowsPrincipal($identity)
        if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
            throw 'Run --repair-windows-installer from an Administrator terminal.'
        }
        # Query MSI registration directly. Win32_Product would repair unrelated products.
        $products = @(
            foreach ($upgrade in @('{70933318-EC5C-487D-9E66-3A981C02AA11}', '{8F04FA95-5EFB-42CB-985E-8F9C40D922FA}')) {
                foreach ($product in $installer.RelatedProducts($upgrade)) {
                    [ordered]@{
                        product_code = $product
                        arch = if ($upgrade -eq '{70933318-EC5C-487D-9E66-3A981C02AA11}') { 'x86_64' } else { 'arm64' }
                        package_code = $installer.ProductInfo($product, 'PackageCode')
                        version = $installer.ProductInfo($product, 'VersionString')
                    }
                }
            }
        )
        [ordered]@{
            cache = Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'Scryer Media\Installer Cache'
            products = $products
        } | ConvertTo-Json -Depth 4 -Compress
    }
    'inspect' {
        $database = $installer.OpenDatabase($env:SCRYER_MSI_PACKAGE, 0)
        [ordered]@{
            product_code = Read-Property $database 'ProductCode'
            package_code = $database.SummaryInformation(0).Property(9)
            version = Read-Property $database 'ProductVersion'
        } | ConvertTo-Json -Compress
    }
    'install' {
        $process = Start-Process -FilePath "$env:SystemRoot\System32\msiexec.exe" -ArgumentList @(
            '/i', ('"' + $env:SCRYER_MSI_PACKAGE + '"'), '/passive', '/norestart',
            '/l*v', ('"' + $env:SCRYER_MSI_PACKAGE + '.install.log"')
        ) -PassThru
        if (-not $process.WaitForExit(1800000)) {
            throw 'Installation is taking longer than expected. Inspect the .install.log before retrying.'
        }
        if ($process.ExitCode -notin @(0, 3010)) {
            throw "Installation failed with exit code $($process.ExitCode). See the .install.log beside the MSI."
        }
        [ordered]@{ reboot_required = ($process.ExitCode -eq 3010) } | ConvertTo-Json -Compress
    }
    'recache' {
        # Verify again immediately before servicing, including the package code:
        # a rebuilt MSI with the same version is not the original installer.
        $database = $installer.OpenDatabase($env:SCRYER_MSI_PACKAGE, 0)
        if ((Read-Property $database 'ProductCode') -ne $env:SCRYER_MSI_PRODUCT_CODE -or
            $database.SummaryInformation(0).Property(9) -ne $env:SCRYER_MSI_PACKAGE_CODE -or
            (Read-Property $database 'ProductVersion') -ne $env:SCRYER_MSI_VERSION) {
            throw 'Recovery installer identity does not match the registered product.'
        }
        if ($installer.ProductInfo($env:SCRYER_MSI_PRODUCT_CODE, 'PackageCode') -ne $env:SCRYER_MSI_PACKAGE_CODE -or
            $installer.ProductInfo($env:SCRYER_MSI_PRODUCT_CODE, 'VersionString') -ne $env:SCRYER_MSI_VERSION) {
            throw 'Installed product changed during recovery. Retry discovery.'
        }
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($database)
        $database = $null
        [GC]::Collect()
        [GC]::WaitForPendingFinalizers()
        # SecureRepair still looks for the registered original filename, even
        # when /fv is passed another filename. Preserve that name inside the
        # verified cache, never by writing to an arbitrary old source directory.
        $name = $installer.ProductInfo($env:SCRYER_MSI_PRODUCT_CODE, 'PackageName')
        if ([string]::IsNullOrWhiteSpace($name) -or $name -ne [IO.Path]::GetFileName($name) -or
            $name.IndexOfAny([IO.Path]::GetInvalidFileNameChars()) -ge 0 -or
            $name -ne $name.TrimEnd(' ', '.') -or
            $name -match '^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(\.|$)') {
            throw 'Registered installer source filename is unsafe.'
        }
        $source = Join-Path (Split-Path -Parent $env:SCRYER_MSI_PACKAGE) $name
        if (-not (Test-Path -LiteralPath $source)) {
            New-Item -ItemType HardLink -Path $source -Target $env:SCRYER_MSI_PACKAGE | Out-Null
        }
        if ((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash -ne
            (Get-FileHash -LiteralPath $env:SCRYER_MSI_PACKAGE -Algorithm SHA256).Hash) {
            throw 'Existing recovery source differs from the verified installer; preserved without modification.'
        }
        # /fv runs from the verified source and recaches the MSI using Windows
        # Installer itself. Never edit LocalPackage or erase installer registry keys.
        $process = Start-Process -FilePath "$env:SystemRoot\System32\msiexec.exe" -ArgumentList @(
            '/fv', ('"' + $source + '"'), '/qn', '/norestart',
            '/l*v', ('"' + $env:SCRYER_MSI_PACKAGE + '.repair.log"')
        ) -PassThru
        if (-not $process.WaitForExit(300000)) {
            throw 'Installer recovery exceeded five minutes; inspect the repair log before retrying.'
        }
        if ($process.ExitCode -notin @(0, 3010)) {
            throw "Installer recovery failed with exit code $($process.ExitCode). See the .repair.log beside the MSI."
        }
        $cached = $installer.ProductInfo($env:SCRYER_MSI_PRODUCT_CODE, 'LocalPackage')
        if (-not (Test-Path -LiteralPath $cached -PathType Leaf)) {
            throw 'Windows Installer did not restore its cached package.'
        }
        [ordered]@{ reboot_required = ($process.ExitCode -eq 3010) } | ConvertTo-Json -Compress
    }
    default { throw 'Unknown installer recovery operation.' }
}
