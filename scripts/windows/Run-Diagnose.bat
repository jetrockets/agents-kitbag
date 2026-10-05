@echo off
chcp 65001 >nul
title Claude Toolkit — Diagnostic
cd /d "%~dp0"
echo.
echo   Claude Toolkit — Diagnostic
echo   ----------------------------
echo   Running. This takes about 15 seconds.
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0diagnose-engine.ps1"
echo.
pause
