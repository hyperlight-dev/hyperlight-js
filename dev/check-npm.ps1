#!/usr/bin/env pwsh

Write-Host "Checking for npm..."

if (Get-Command npm -ErrorAction SilentlyContinue) {
    $version = npm --version
    Write-Host "✓ npm is installed: $version" -ForegroundColor Green
    exit 0
}

Write-Host "❌ npm is not installed!" -ForegroundColor Red
Write-Host ""

if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
    Write-Host "❌ winget is not available. Please install Node.js from: https://nodejs.org/" -ForegroundColor Red
    exit 1
}

Write-Host "Detected: Windows"
Write-Host "Install command: winget install OpenJS.NodeJS"
Write-Host ""

if ([System.Console]::IsInputRedirected -eq $false) {
    $response = Read-Host "Install npm automatically? [y/N]"
    if ($response -notmatch '^[Yy]$') {
        Write-Host "❌ Installation cancelled. Please install npm manually:" -ForegroundColor Red
        Write-Host "   winget install OpenJS.NodeJS"
        exit 1
    }
} else {
    Write-Host "🤖 Non-interactive terminal detected - installing automatically..." -ForegroundColor Cyan
}

Write-Host "📦 Installing npm..." -ForegroundColor Yellow
winget install OpenJS.NodeJS --silent --accept-source-agreements --accept-package-agreements

if ($LASTEXITCODE -eq 0) {
    Write-Host ""
    $version = npm --version
    Write-Host "✅ npm installed successfully: $version" -ForegroundColor Green
} else {
    Write-Host ""
    Write-Host "❌ Installation failed. Please install manually:" -ForegroundColor Red
    Write-Host "   winget install OpenJS.NodeJS"
    exit 1
}
