#Requires -Version 5.1
<#
.SYNOPSIS
    把 FFmpeg / FFprobe 下载到 server\binaries\，供本机转换服务使用。

.DESCRIPTION
    仓库不携带 ffmpeg.exe / ffprobe.exe（两个加起来约 140 MB，不适合放进 Git）。
    克隆之后跑一次本脚本补齐，转换功能和集成测试才能工作。

    脚本从官方压缩包里取出 bin\ffmpeg.exe 与 bin\ffprobe.exe，另外把压缩包根目录的
    LICENSE（GPLv3 全文）与 README.txt（精确版本、源码 commit、完整 configure 参数）
    一起放进 server\binaries\ —— 分发绿色版时要附上这些，见 THIRD-PARTY.md。
    不整包解压，也不会动 server\binaries\ 里的其它东西。

    默认取 gyan.dev 的 release essentials 构建。判断依据：官方 Essentials
    构建自带 x264 / x265 / libvpx / libwebp 等常用编解码器，够这个项目用。

.PARAMETER Url
    压缩包地址。默认 gyan.dev 的最新 release essentials（zip，约 115 MB）。
    需要锁定版本时传 GitHub 镜像的具体 tag，例如：
      -Url https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip

.PARAMETER Sha256
    期望的 SHA-256。给了就校验，不匹配直接报错退出。

.PARAMETER From
    直接用本地已有的压缩包（离线 / 内网场景），此时忽略 -Url，不联网。
    只支持 .zip；.7z 需要自行解压，把 bin\ 下的两个 exe 连同压缩包里的
    LICENSE 与 README.txt 一并手动放到 server\binaries\。

.PARAMETER Force
    目标文件已存在时也重新下载覆盖。默认已存在就跳过。

.PARAMETER KeepArchive
    保留下载的压缩包（默认用完删掉，省 115 MB 临时空间）。

.EXAMPLE
    pwsh scripts/fetch-ffmpeg.ps1

.EXAMPLE
    # 内网：把压缩包拷进来再跑
    pwsh scripts/fetch-ffmpeg.ps1 -From D:\downloads\ffmpeg-release-essentials.zip

.EXAMPLE
    # 锁版本 + 校验
    pwsh scripts/fetch-ffmpeg.ps1 `
      -Url https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip `
      -Sha256 60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba
#>
[CmdletBinding()]
param(
    [string] $Url = 'https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip',
    [string] $Sha256,
    [string] $From,
    [switch] $Force,
    [switch] $KeepArchive
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ── 路径 ────────────────────────────────────────────────────────────────

$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root 'server\binaries'
$wanted = @('ffmpeg.exe', 'ffprobe.exe')

# 一并取出的说明文件：压缩包内路径模式 → 本地文件名。
# GPLv3 要求分发二进制时附上许可证全文并给出对应源码，这两个文件就是凭据。
$notices = [ordered]@{
    'README\.txt' = 'FFMPEG-BUILD-INFO.txt'
    'LICENSE'     = 'FFMPEG-LICENSE-GPLv3.txt'
}

function Write-Step($message) { Write-Host "==> $message" -ForegroundColor Cyan }
function Write-Ok($message) { Write-Host "    $message" -ForegroundColor Green }
function Write-Warn($message) { Write-Host "    $message" -ForegroundColor Yellow }

# ── 已经有就不用再下 ────────────────────────────────────────────────────

$missing = @($wanted | Where-Object { -not (Test-Path -LiteralPath (Join-Path $dest $_)) })
if ($missing.Count -eq 0 -and -not $Force) {
    Write-Step '引擎已就位，无需下载'
    foreach ($name in $wanted) {
        $file = Get-Item -LiteralPath (Join-Path $dest $name)
        Write-Ok ('{0,-12} {1,8:N1} MB' -f $file.Name, ($file.Length / 1MB))
    }
    $absent = @($notices.Values | Where-Object { -not (Test-Path -LiteralPath (Join-Path $dest $_)) })
    if ($absent.Count -gt 0) {
        Write-Host ''
        Write-Warn "缺少许可 / 构建说明文件：$($absent -join '、')"
        Write-Warn '分发绿色版前需要补齐 —— GPLv3 要求随二进制附许可证全文并给出对应源码，'
        Write-Warn '详见 THIRD-PARTY.md。补齐方式（会重新下载一次，覆盖现有引擎）：'
        Write-Warn '  pwsh scripts/fetch-ffmpeg.ps1 -Force'
    } else {
        Write-Host '    要强制重新下载，加 -Force。'
    }
    return
}

if (-not (Test-Path -LiteralPath $dest)) {
    New-Item -ItemType Directory -Path $dest -Force | Out-Null
}

# ── 准备压缩包：本地 or 下载 ────────────────────────────────────────────

$archive = $null
$downloaded = $false

if ($From) {
    $archive = (Resolve-Path -LiteralPath $From).Path
    if ([System.IO.Path]::GetExtension($archive) -ne '.zip') {
        throw "只支持 .zip（收到 $([System.IO.Path]::GetExtension($archive))）。.7z 请先自行解压，把 bin\ffmpeg.exe 与 bin\ffprobe.exe 放到 $dest。"
    }
    Write-Step "使用本地压缩包 $archive"
} else {
    $archive = Join-Path ([System.IO.Path]::GetTempPath()) "lythara-ffmpeg-$([guid]::NewGuid().ToString('N').Substring(0, 8)).zip"
    $downloaded = $true

    Write-Step "下载 $Url"
    Write-Host '    约 115 MB，视网速可能要几分钟。' -ForegroundColor DarkGray

    $curl = Get-Command curl.exe -ErrorAction SilentlyContinue
    if ($curl) {
        & curl.exe --location --fail --retry 2 --output $archive $Url
        if ($LASTEXITCODE -ne 0) {
            throw "下载失败（curl 退出码 $LASTEXITCODE）。可换网络重试，或手动下载后用 -From 指向本地压缩包。"
        }
    } else {
        # Windows PowerShell 5.1 默认可能只开 TLS 1.0，先抬到 1.2。
        if ($PSVersionTable.PSVersion.Major -lt 6) {
            [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        }
        $previous = $ProgressPreference
        $ProgressPreference = 'SilentlyContinue'   # 进度条会让大文件下载慢好几倍
        try {
            Invoke-WebRequest -Uri $Url -OutFile $archive
        } finally {
            $ProgressPreference = $previous
        }
    }
    Write-Ok ('已下载 {0:N1} MB' -f ((Get-Item -LiteralPath $archive).Length / 1MB))
}

try {
    # ── 校验 ────────────────────────────────────────────────────────────

    if ($Sha256) {
        Write-Step '校验 SHA-256'
        $actual = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash
        if ($actual -ne $Sha256.ToUpperInvariant()) {
            throw "校验不通过。期望 $($Sha256.ToUpperInvariant())，实际 $actual"
        }
        Write-Ok '校验通过'
    }

    # ── 取 bin\ 下的两个 exe，外加许可与构建说明 ────────────────────────

    Write-Step "解出 $($wanted -join ' / ')"

    Add-Type -AssemblyName System.IO.Compression.FileSystem -ErrorAction SilentlyContinue
    $zip = [System.IO.Compression.ZipFile]::OpenRead($archive)
    $missingNotices = @()
    try {
        foreach ($name in $wanted) {
            $entry = $zip.Entries |
                Where-Object { $_.FullName -match "(^|/)bin/$([regex]::Escape($name))$" } |
                Select-Object -First 1
            if (-not $entry) {
                throw "压缩包里找不到 bin\$name，可能下载到的不是 FFmpeg 构建包。"
            }
            $target = Join-Path $dest $name
            [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $true)
            Write-Ok ('{0,-12} {1,8:N1} MB' -f $name, ((Get-Item -LiteralPath $target).Length / 1MB))
        }

        # 许可与构建说明：优先匹配「一级目录/文件名」，扁平结构的包再退回根级匹配。
        foreach ($pattern in $notices.Keys) {
            $entry = $zip.Entries | Where-Object { $_.FullName -match "^[^/]+/$pattern$" } | Select-Object -First 1
            if (-not $entry) {
                $entry = $zip.Entries | Where-Object { $_.FullName -match "^$pattern$" } | Select-Object -First 1
            }
            if ($entry) {
                $target = Join-Path $dest $notices[$pattern]
                [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $true)
                Write-Ok ('{0,-24} {1,6:N0} KB  ← 压缩包内 {2}' -f $notices[$pattern], ((Get-Item -LiteralPath $target).Length / 1KB), $entry.FullName)
            } else {
                $missingNotices += ($pattern -replace '\\', '')
            }
        }
    } finally {
        $zip.Dispose()
    }

    if ($missingNotices.Count -gt 0) {
        Write-Warn "压缩包里没找到：$($missingNotices -join '、')"
        Write-Warn '引擎已经能跑，但分发绿色版前要自行补上许可证全文与源码信息，见 THIRD-PARTY.md。'
    }

    # ── 报一下版本，确认真的能跑 ────────────────────────────────────────
    # 跑不起来不算致命（文件已经落位），但要明确说出来，别让人以为一切正常。

    Write-Step '确认可执行'
    $ffmpeg = Join-Path $dest 'ffmpeg.exe'
    try {
        $version = (& $ffmpeg -version 2>&1 | Select-Object -First 1)
        if ($LASTEXITCODE -eq 0 -and $version) {
            Write-Ok $version
        } else {
            Write-Warn "ffmpeg.exe 执行返回异常（退出码 $LASTEXITCODE），请手动确认：$ffmpeg -version"
        }
    } catch {
        Write-Warn "无法执行 ffmpeg.exe（$($_.Exception.Message)），请手动确认：$ffmpeg -version"
    }

    Write-Host ''
    Write-Host "引擎已就位：$dest" -ForegroundColor Green
    Write-Host '下一步：npm install && npm run dist' -ForegroundColor Green
} finally {
    if ($downloaded -and -not $KeepArchive -and (Test-Path -LiteralPath $archive)) {
        Remove-Item -LiteralPath $archive -Force
    } elseif ($downloaded) {
        Write-Host "    压缩包保留在 $archive"
    }
}
