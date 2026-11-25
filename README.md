# on99KB

A resizable, always-on-top Windows on-screen keyboard for the classic IBM **CP437** / DOS Alt+numpad characters (Alt+1 through Alt+255).

Clicking a key types that character into the **previously focused** window. The keyboard itself does not steal focus, so you can keep Notepad, a browser, or a terminal focused and use this tool to enter symbols such as ☺ ♥ ⌂ █ α.

點擊按鍵會把該字元輸入到**先前聚焦**的視窗。鍵盤本身不會搶走焦點。

## Features

- Native Win32 GUI (Rust + [`windows`](https://crates.io/crates/windows) crate). No webview, no admin rights, no telemetry, no hooks.
- All **255** CP437 glyphs (codes 1..=255). Code 32 is a normal space (`空格`); code 255 is a non-breaking space U+00A0 (`不換行空格`).
- Resizable window: the 16×16 key grid scales with the window.
- Always on top (`WS_EX_TOPMOST`) and non-activating (`WS_EX_NOACTIVATE` + `WM_MOUSEACTIVATE` / `MA_NOACTIVATE`).
- Types the actual Unicode glyph with `SendInput` and `KEYEVENTF_UNICODE` (not a simulated Alt+numpad sequence).
- Each key shows the glyph and the `Alt+n` code, so it also works as an Alt-code reference.
- Fonts: Segoe UI Symbol for glyphs, Microsoft JhengHei for Chinese labels, Segoe UI for captions (all built into Windows).

## Run the exe

Download `on99KB.exe` from the [Releases](https://github.com/TimothyLam1017/on99KB/releases) page, or build it yourself.

1. Double-click `on99KB.exe`. A small tool window stays on top.
2. Click the window you want to type into (Notepad, a browser, a terminal, …) so it has focus.
3. Click a key on on99KB. The character is inserted into that focused window.
4. Drag any window edge or corner to resize the keyboard; the keys scale automatically.
5. Close it with the window’s close button.

The first grid cell (`on99KB`) is a label only and does not type anything.

## Build on Windows

Requirements: [Rust](https://rustup.rs/) (stable) and the MSVC or GNU toolchain.

```bat
git clone https://github.com/TimothyLam1017/on99KB.git
cd on99KB
cargo test --lib
cargo build --release
```

The exe is `target\release\on99KB.exe`.

## Cross-compile from Linux

Requirements: Rust stable, the `x86_64-pc-windows-gnu` target, and MinGW-w64.

```bash
rustup default stable
rustup target add x86_64-pc-windows-gnu
# Debian / Ubuntu:
sudo apt-get install -y mingw-w64

git clone https://github.com/TimothyLam1017/on99KB.git
cd on99KB
cargo test --lib
cargo build --release --target x86_64-pc-windows-gnu
```

The exe is `target/x86_64-pc-windows-gnu/release/on99KB.exe`.

This repository’s `.cargo/config.toml` points the GNU target at `x86_64-w64-mingw32-gcc` and passes `-static` so the exe does not depend on MinGW runtime DLLs (`libgcc_s_seh-1.dll`, `libwinpthread-1.dll`, and so on).

## Character set

The layout is a 16×16 grid (256 cells). Cell 0 is a title cell. Cells 1–255 are CP437:

| Code | Character | Notes |
|------|-----------|--------|
| 1 | ☺ | |
| 3 | ♥ | |
| 32 | space | labelled 空格 |
| 65 | A | |
| 127 | ⌂ | |
| 219 | █ | |
| 224 | α | |
| 255 | U+00A0 | labelled 不換行空格 |

The mapping is covered by unit tests against the full requested list (`cargo test --lib`).

## Limits

- **Elevated / administrator apps:** Windows integrity rules (UIPI) block `SendInput` from a medium-integrity process into a higher-integrity window. If the target is running as administrator, run on99KB elevated too, or type into a non-elevated window.
- Some games or apps that use raw input / exclusive keyboard capture may ignore Unicode `SendInput`.
- Glyph appearance depends on the fonts installed on the PC. Windows includes Segoe UI Symbol, which covers these CP437 characters.

## License

[MIT](LICENSE) © 2026 Timothy Lam
