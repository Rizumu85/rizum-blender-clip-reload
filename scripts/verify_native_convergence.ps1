param(
    [string[]]$Samples = @(
        "Test_Clipping",
        "Test_ClippingEdge",
        "Test_AddGlowMultiply",
        "Test_ToneCurve"
    ),
    [switch]$SkipClipCompare
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$nativeRust = Join-Path $repoRoot "native/rust"
$clipCli = Join-Path $nativeRust "target/debug/clip_cli.exe"

function Invoke-Step {
    param(
        [string]$Name,
        [scriptblock]$Command
    )

    Write-Host "==> $Name"
    & $Command
}

Push-Location $repoRoot
try {
    Invoke-Step "cargo fmt --all --check" {
        Push-Location $nativeRust
        try {
            cargo fmt --all --check
        } finally {
            Pop-Location
        }
    }

    Invoke-Step "cargo test --workspace" {
        Push-Location $nativeRust
        try {
            cargo test --workspace
        } finally {
            Pop-Location
        }
    }

    Invoke-Step "python unittest" {
        python -m unittest discover -s tests
    }

    Invoke-Step "build clip_cli debug binary" {
        Push-Location $nativeRust
        try {
            cargo build -q -p clip_cli
        } finally {
            Pop-Location
        }
    }

    foreach ($sample in $Samples) {
        $clip = Join-Path $repoRoot "img/$sample.clip"
        if (!(Test-Path $clip)) {
            Write-Host "skip missing clip sample: $sample"
            continue
        }

        Invoke-Step "performance-plan smoke: $sample" {
            Push-Location $nativeRust
            try {
                $jsonText = & $clipCli $clip --performance-plan-json
                $json = $jsonText | ConvertFrom-Json
                if ($null -eq $json.coverage) {
                    throw "performance plan missing coverage object"
                }
                if ($null -eq $json.fallback) {
                    throw "performance plan missing fallback object"
                }
                if ($null -eq $json.coverage.legacy_segment_count) {
                    throw "performance plan missing coverage.legacy_segment_count"
                }
                if ($null -eq $json.coverage.top_barrier_reasons) {
                    throw "performance plan missing coverage.top_barrier_reasons"
                }
            } finally {
                Pop-Location
            }
        }

        if ($SkipClipCompare) {
            continue
        }

        $png = Join-Path $repoRoot "img/$sample.png"
        if (!(Test-Path $png)) {
            Write-Host "skip missing PNG reference: $sample"
            continue
        }

        Invoke-Step "clip_cli compare smoke: $sample" {
            Push-Location $nativeRust
            try {
                & $clipCli $clip --compare-png $png
            } finally {
                Pop-Location
            }
        }
    }
} finally {
    Pop-Location
}
