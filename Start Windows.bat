@echo off
chcp 65001 >nul
title Claude MCP Toolkit

REM Hot path: skip PowerShell entirely when node + deps are already installed.
REM Spawning powershell.exe inside the same console makes conhost (in ForceV2
REM mode) switch the window font to PowerShell's per-exe setting (Lucida
REM Console), which looks dated. Calling node directly keeps the cmd default
REM (Consolas) and shaves a process from startup.
set "_NEED_SETUP=0"
where node >nul 2>nul
if errorlevel 1 set "_NEED_SETUP=1"
if not exist "%~dp0node_modules\inquirer\package.json" set "_NEED_SETUP=1"

if "%_NEED_SETUP%"=="1" (
    powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\windows\setup.ps1"
    if errorlevel 1 (
        pause
        exit /b 1
    )
    REM PATH inside the powershell child doesn't propagate back. Prepend the
    REM install dir so `node` below is found.
    if exist "%ProgramFiles%\nodejs\node.exe" set "PATH=%ProgramFiles%\nodejs;%PATH%"
)

if not exist "%~dp0src\index.js" (
    echo.
    echo   ERROR: src\index.js not found. The download may be incomplete.
    echo   Please re-download and extract the full ZIP archive.
    echo.
    pause
    exit /b 1
)

node "%~dp0src\index.js"
pause
