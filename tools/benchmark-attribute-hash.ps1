# Copyright The OpenTelemetry Authors
# SPDX-License-Identifier: Apache-2.0

param([ValidateRange(10, 10000)][int]$SampleSize = 30)

$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot '../rust/otap-dataflow')
try {
    & cargo test -p otel-arrow-dfe-pdata --lib hash
    if ($LASTEXITCODE -ne 0) { throw 'Baseline hash tests failed' }
    & cargo test -p otel-arrow-dfe-pdata --lib --features experimental-dictionary-hash hash
    if ($LASTEXITCODE -ne 0) { throw 'Optimized hash tests failed' }
    & cargo bench -p otel-arrow-dfe-pdata --bench attribute_hash -- --sample-size $SampleSize --save-baseline before
    if ($LASTEXITCODE -ne 0) { throw 'Baseline benchmark failed' }
    & cargo bench -p otel-arrow-dfe-pdata --features experimental-dictionary-hash --bench attribute_hash -- --sample-size $SampleSize --baseline before
    if ($LASTEXITCODE -ne 0) { throw 'Optimized benchmark failed' }

    $metadata = & cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed' }
    $results = Join-Path $metadata.target_directory 'criterion/attribute_hash'
    $summary = @(Get-ChildItem $results -Filter benchmark.json -Recurse | ForEach-Object {
        $benchmark = Get-Content $_.FullName -Raw | ConvertFrom-Json
        if ($benchmark.full_id -match '^attribute_hash/(u16|u8|plain)/(\d+)/(10000)$') {
            $encoding = $Matches[1]
            $distinct = [int]$Matches[2]
            $rows = [int]$Matches[3]
            $before = (Get-Content (Join-Path $_.DirectoryName '../before/estimates.json') -Raw | ConvertFrom-Json).mean
            $after = (Get-Content (Join-Path $_.DirectoryName '../new/estimates.json') -Raw | ConvertFrom-Json).mean
            [pscustomobject]@{
                encoding = $encoding
                distinct_values = $distinct
                rows = $rows
                before_us = $before.point_estimate / 1000
                before_lower_us = $before.confidence_interval.lower_bound / 1000
                before_upper_us = $before.confidence_interval.upper_bound / 1000
                after_us = $after.point_estimate / 1000
                after_lower_us = $after.confidence_interval.lower_bound / 1000
                after_upper_us = $after.confidence_interval.upper_bound / 1000
                speedup = $before.point_estimate / $after.point_estimate
            }
        }
    })
    if ($summary.Count -eq 0) { throw 'No attribute hash measurements found' }
    Write-Host "`nMean batch latency (microseconds); speedup > 1 means faster:"
    $summary | Sort-Object encoding, distinct_values -Unique | ConvertTo-Csv -NoTypeInformation
} finally {
    Pop-Location
}