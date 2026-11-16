# Programs docmap-desktop starts

Every program the window starts, with why, how long it lives and which way of
[appkit's `docs/PROCESSES.md`](https://github.com/WKDSteve/appkit/blob/main/docs/PROCESSES.md)
(rule `RS-08`) starts it. **A process that is not in this list is a bug.** The list is taken
from the code (`tools/process-starts.allow`, the calls of `src-tauri/src/proc.rs`, which is
`appkit_core::child`, and of `appkit_core::launch`) and checked against a measurement with
`app-proc-watch`. For the buttons behind each start see
[USAGE.md, "Programs this window starts"](USAGE.md#programs-this-window-starts).

The ways:

- **child**: `proc::Cmd`, which starts through `appkit_core::child`. No console window, the
  output is read by the window and capped, a time limit, and the child is bound to the window by
  the system (a job object on Windows, a process group elsewhere): it ends with the window, also
  when the window crashes or is killed, together with everything it started. The first start of
  each kind is written to `processes.log` in the app's log folder. `git` and `gh` are told never
  to prompt.
- **launch**: `appkit_core::launch::spawn`. A program that is yours, not the window's: it gets
  its own window and outlives the window on purpose; nothing of the window is inherited.
- **system**: the Windows shell opens something (`ShellExecuteExW`, `SHOpenFolderAndSelectItems`
  in `src-tauri/src/shellopen.rs`); no process of the window is started. On macOS and Linux
  `/usr/bin/open` or `xdg-open` is started through **launch**.

| Program | Why | How long | Way |
| --- | --- | --- | --- |
| the engine `docmap --capabilities` | what the engine file can do, once per engine file | the call, at most 20 seconds | child |
| the engine `docmap` (generate, check) | **Generate**, **Check** and the Generate commands of the menu | the call, at most 30 minutes | child |
| the engine `docmap` (the map page's panels, the rules agent) | the panels of a map, a rules run | the call, at most 2 minutes | child |
| the engine `docmap` (the Rules tab) | the Rules tab of a project | the call, at most 2 minutes and a memory limit (`Start::memory`) | child |
| `nvim --headless` | **Import from the Neovim config**, switching telemetry, **Ask Neovim** in the traffic settings | the call, at most 60 seconds (30 seconds for Ask Neovim) | child |
| `git clone --depth 1` | **Clone** in the URL tab | at most 15 minutes, or **Stop** (the half-cloned folder is removed) | child |
| `gh repo list` | **List my GitHub repositories** in the URL tab | the call, at most 60 seconds | child |
| your editor command (`code`, `nvim {file}`, …) | **Open in editor** | yours: it outlives the window | named exception: launch with a console of its own (`Window::NewConsole`); a terminal editor is a window you work in, a windowed editor ignores the console; a `.cmd` wrapper shows its console for as long as it runs |
| the browser, the file manager, the program of a file type | **Open map in browser**, **Reveal in file manager**, the documentation, a link | yours | system |
| `msedgewebview2.exe` (several) | the WebView2 runtime draws the window | as long as the window; they end within a second after it | started by WebView2, not by the app's code |
| `conhost.exe` | Windows gives every console program (`docmap`, `nvim`, `git`, `gh`) a console host, also without a window | as long as its console program | comes with each child; no window |

What the window does **not** start any more: `explorer.exe` (the shell is asked directly),
`git remote get-url origin` (the origin is read from the repository's config files), `cmd.exe`,
`tasklist.exe` or `taskkill.exe`.
