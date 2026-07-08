# One-time rename pass for hermes -> allo copied sources.
param(
    [string]$Root = (Join-Path $PSScriptRoot "..")
)

$ErrorActionPreference = "Stop"
Set-Location $Root

$targets = @(
    "crates\backend\nomifun-cloud",
    "crates\agent\nomi-poi",
    "crates\agent\nomi-insights-core",
    "crates\agent\nomi-media",
    "crates\agent\nomi-config\src\server.rs",
    "crates\agent\nomi-config\src\interest.rs",
    "crates\agent\nomi-config\src\insights.rs",
    "crates\agent\nomi-config\src\media.rs"
)

function Replace-InFile([string]$path) {
    if (-not (Test-Path $path)) { return }
    $c = [IO.File]::ReadAllText($path)
    $pairs = @(
        @('hermes_config::', 'nomi_config::'),
        @('hermes_core::', 'nomi_types::'),
        @('hermes_agent::', 'nomi_agent::'),
        @('hermes_insights::', 'nomi_insights_core::'),
        @('hermes_intelligence::', 'nomi_insights_core::'),
        @('hermes_skills::', 'nomi_skills::'),
        @('hermes_tools::', 'nomi_tools::'),
        @('hermes_media_workflows::', 'nomi_media::'),
        @('hermes_auth::', 'crate::token_store::'),
        @('hermes_home', 'data_dir'),
        @('HERMES_', 'NOMIFUN_'),
        @('hermes-agent-ultra', 'nomifun'),
        @('use nomi_config;', 'use nomi_config;')
    )
    foreach ($p in $pairs) {
        $c = $c.Replace($p[0], $p[1])
    }
    [IO.File]::WriteAllText($path, $c)
}

foreach ($t in $targets) {
    if (Test-Path $t -PathType Container) {
        Get-ChildItem $t -Recurse -Filter *.rs | ForEach-Object { Replace-InFile $_.FullName }
    } elseif (Test-Path $t -PathType Leaf) {
        Replace-InFile (Resolve-Path $t).Path
    }
}

Write-Host "Rename pass complete."
