---
title: Report a problem
description: What to include when you report a DeckCraft bug, and where to post it.
order: 30
verified: 0.7.0
---

Post bugs in the [issue tracker](https://github.com/storytold/deckcraft/issues) or ask in [Discord](https://discord.gg/artcraft). Include the version, your system, the steps and the log file.

## What to include

1. The DeckCraft version. Open the command palette (Ctrl+Shift+P; on macOS, use Cmd in place of Ctrl), run **About DeckCraft** and read the "Version" line.
2. Your system, for example Windows 11 and your graphics card.
3. What you did, what you expected and what happened.
4. The log file, `%APPDATA%\DeckCraft\logs\deckcraft.log`. On macOS it is `~/Library/Application Support/DeckCraft/logs/`, on Linux `~/.config/deckcraft/logs/`. It holds crash reports, failed crash-recovery saves, PowerPoint parts skipped on import and audio output problems. Each start moves the previous log to `deckcraft.1.log` and that one to `deckcraft.2.log`, so the log of a run that crashed is still there after the next start.
5. For a file that opens wrong, the file itself if you can share it.

For more detail in the log, run `$env:RUST_LOG = "debug"` in PowerShell and start `deckcraft.exe` from the same window.

## Still stuck?

Ask in [Discord](https://discord.gg/artcraft).
