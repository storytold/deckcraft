---
title: DeckCraft closes right after it starts
description: On Windows 11 the DeckCraft window appears and closes at once; start it with the Vulkan graphics backend instead.
order: 10
verified: 0.7.0
---

If DeckCraft flashes and quits, start it from PowerShell with `WGPU_BACKEND` set to `vulkan`. On Intel graphics, use `gl` instead.

## Symptoms

- The app shows up in Task Manager for a moment, or its window opens and closes at once.
- `deckcraft.log` ends with lines like `Exiting because of error: WGPU error: Parent device is lost`.

## Fix

1. Open the folder that contains `deckcraft.exe`. For the installer that is the `DeckCraft` folder inside Program Files. For the portable zip it is the folder you unzipped.
2. Open PowerShell in that folder.
3. Run these lines one at a time:

        $env:WGPU_BACKEND = "vulkan"
        & .\deckcraft.exe
        Remove-Item Env:WGPU_BACKEND

   The last line resets the setting so later launches use the default again.

**Intel graphics (UHD, Iris):** use `gl` in place of `vulkan`. Intel's Vulkan driver (`igvk64.dll`) can crash DeckCraft at start ([issue 59](https://github.com/storytold/deckcraft/issues/59)).

On AMD graphics do not set `WGPU_BACKEND` to `gl`: it can bring back the `atio6axx.dll` driver crash.

## Why this happens

Two causes are documented. Vulkan works around the first one except on Intel graphics.

- **Missing DirectX shader compiler files.** Issue 153 reports that the 0.5.0 build, in both the MSI and the portable zip, ships neither `dxcompiler.dll` nor `dxil.dll`, and the 0.7.0 portable zip does not have them either. DirectX 12 shader compilation then fails and the graphics device is lost before the first frame, which is where `Parent device is lost` comes from. The reporter saw this on an NVIDIA PC on Windows 11 and confirmed that `WGPU_BACKEND=vulkan` starts the app. A fix, pull request 176 ("Windows: compile DX12 shaders with FXC"), is merged but is not in 0.7.0.
- **An OpenGL crash in some AMD drivers.** The README says the Windows app starts the graphics layer on DirectX 12 only by default, because also creating an OpenGL instance can crash some drivers (AMD's `atio6axx.dll`) before the window appears.

## Find the log

The log is `%APPDATA%\DeckCraft\logs\deckcraft.log`. Attach it when you [report a problem](send-a-bug-report.md).

## Still stuck?

Ask in [Discord](https://discord.gg/artcraft) or open an issue in the [issue tracker](https://github.com/storytold/deckcraft/issues). Attach the log.
