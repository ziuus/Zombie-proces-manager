# Learning OPM: A Step-by-Step Guide

Welcome to the OPM tutorial. This guide will help you understand how to use and extend the Orphan Process Manager.

## 🛠️ Step 1: Installation & Setup

Ensure you have the Rust toolchain installed.

```bash
git clone https://github.com/ziuus/opm.git
cd opm
cargo build --release
```

## 🔍 Step 2: Understanding "Orphans"

In Linux, when a parent process dies, its children are inherited by PID 1 (`systemd` or `init`). OPM scans the system for these specific processes.

Run OPM to see what's currently "orphaned" on your system:
```bash
cargo run --release
```

## 🖥️ Step 3: Navigating the TUI

- Use `j` and `k` to scroll through the list.
- Look at the **Memory MB** column to see which processes are the heaviest.
- Look at the **Ports** column to see which process is blocking your `localhost:3000`.
- Press `Tab` to cycle between view modes: **ORPHANS** (default), **ALL PROCS**, and **TRACKED**.

## 🚀 Step 4: Launching Apps

OPM can also launch and track your own processes:

1. Press `a` to open the launch input popup.
2. Type any shell command — `brave`, `npm run dev`, `code`, `ncmpcpp`, anything.
3. Press `Enter` to launch, or `Esc` to cancel.
4. OPM spawns the process, tracks its entire process tree (children, grandchildren, etc.), and automatically switches to TRACKED view.
5. Tracked processes appear in **green** (bold green for the root process you launched).

## 💀 Step 5: Reclaiming Resources

When you find a process that shouldn't be running:

1. Select it with `j`/`k` or the arrow keys.
2. Press `Enter` or `x`.
3. For tracked root processes, all children are killed too.
4. The list refreshes automatically.

## 🔄 Step 6: Auto-Cleanup

OPM automatically:
- Refreshes the process list every 5 seconds.
- Detects when a tracked process ends and shows a status message.
- Removes dead tracked entries from the list.

## 🚀 Next Steps: Customizing Detection

You can add more process names to the orphan detection list in `src/main.rs`. Look for the `suspicious_names` array and add your own tools:

```rust
let suspicious_names = [
    "node", "python", "ruby", "java", "chrome", "chromium", "brave",
    "esbuild", "vite", "next-server", "webpack", "docker", "rustc",
    // add yours here
];
```

## Key Controls Summary

| Key     | Action                    |
|---------|---------------------------|
| `a`     | Launch a new app/command  |
| `Tab`   | Cycle view mode           |
| `j`/`k` | Navigate list             |
| `x`/Enter | Kill selected process   |
| `r`     | Manual refresh            |
| `q`/Esc | Quit                      |
