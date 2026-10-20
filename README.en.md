<div align="center">

# Seek

**Find any file on your computer in milliseconds.**

🇧🇷 [Versão em português](README.md)

File search for Windows, made for people who have millions of files and can't remember where they put one.

[![Versão](https://img.shields.io/github/v/release/inflzzz/seek?label=vers%C3%A3o&color=2ea44f)](https://github.com/inflzzz/seek/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/inflzzz/seek/total?label=downloads&color=0969da)](https://github.com/inflzzz/seek/releases)
![Windows 10 e 11](https://img.shields.io/badge/Windows-10%20%7C%2011-0078d4?logo=windows&logoColor=white)
![Feito em Rust](https://img.shields.io/badge/feito%20em-Rust-b7410e?logo=rust&logoColor=white)

### Download Seek.exe from the **Releases** tab of this repository

A single file. Nothing to install. It updates itself.

<br>

<img src="docs/demo.gif" alt="Seek demo: search, thumbnails, preview, duplicates and simple mode" width="760">

<sub>Prefer video? <a href="docs/demo.mp4">docs/demo.mp4</a></sub>

</div>

---

## Why Seek

- **Instant.** Search happens in memory: a few milliseconds, even with 1 million files.
- **Reads the whole disk in seconds.** As administrator, Seek reads the NTFS internal table (the MFT) and indexes 1 million files in about 6 seconds.
- **Always up to date.** It watches the disk live: a file created, deleted or renamed shows up in search right away.
- **Lives in the tray.** Closing the window doesn't quit the program. **Ctrl + Space**, from anywhere, opens the small search window.
- **Find it by what you remember.** By name, by extension (any of them), by type, by size, by date, and even by **similar photo** or **similar icon**.
- **Private.** Everything runs on your computer. Seek reads names, folders, sizes and dates. Nothing is sent to the internet.

## See it in action

<table>
  <tr>
    <td width="50%"><img src="docs/screenshots/01-fotos.png" alt="Photos in a grid with thumbnails"><br><b>Photos with thumbnails</b><br>List, small, medium or large. Videos and phone photos (HEIC) too.</td>
    <td width="50%"><img src="docs/screenshots/02-busca.png" alt="Search with the matched part highlighted"><br><b>Search with highlighting</b><br>The matched part is marked in the name, and the list shows folder, size and date.</td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/03-duplicados.png" alt="Duplicate files grouped together"><br><b>Find duplicates</b><br>Compares the content, groups the copies and shows how much space you can free up. It only moves, never deletes.</td>
    <td><img src="docs/screenshots/04-previa.png" alt="Full-screen preview"><br><b>Quick preview</b><br>F3 shows the image, the video or the beginning of the text. The arrow keys switch files.</td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/05-modo-simples.png" alt="Simple mode"><br><b>Simple mode</b><br>Just the search and one line per file, Everything-style.</td>
    <td><img src="docs/screenshots/07-janelinha.png" alt="Small search window opened by the shortcut"><br><b>Small window with Ctrl + Space</b><br>Opens from any program, Enter opens the first result, Esc hides it.</td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/06-tema-claro.png" alt="Light theme"><br><b>Dark or light theme</b></td>
    <td><img src="docs/screenshots/08-configuracoes.png" alt="Settings"><br><b>Settings</b><br>Change the shortcut, the theme and open Seek together with Windows.</td>
  </tr>
</table>

## Getting started

1. Download `Seek.exe` from the **Releases** tab of this repository and open it.
2. Click **Escolher disco** (or **Todos os discos**) and wait for the first read.
3. Type. Done.

> **Windows showed "unknown app"?** Seek doesn't have a digital signature yet, so SmartScreen warns the first time. Click **More info → Run anyway**.

## What you can search for

| Type | Finds |
|---|---|
| `curriculo` | Files with that word in the name or in the folder |
| `foto praia` | Photos with `praia` in the name or in the folder (all words must appear) |
| `.png`, `.xyz` | Only that extension. **Any extension works**, even rare ones |
| `vídeo maior:100MB` | Videos larger than 100 MB (`menor:` for the opposite) |
| `modificado:hoje` | Changed today. Also `ontem`, `semana`, `mes`, `ano`, `2023`, `2023-05` |
| `modificado:2023-01..2023-06` | Date range. Also `>2023-01-01` and `<2023`. `criado:` works the same way |
| `app`, `código`, `design`, `dados`, `fonte` | Whole categories (13 in total) |
| `foto -ext:png` | Photos, except PNG |
| `.jpg \| .png` | JPG **or** PNG |
| `"viagem praia"` | The exact phrase |

The keywords inside a query are in Portuguese, and you type them exactly as shown: `maior:` (larger), `menor:` (smaller), `modificado:` (modified), `criado:` (created), and the category words `vídeo`, `foto` and `app`. Upper case and accents don't matter. The **Extensões** list on the side shows every extension that exists on your disk, with the count for each one, and suggests as you type `.p`.

## Shortcuts

| Shortcut | What it does |
|---|---|
| **Ctrl + Space** | Opens the small search window from anywhere (you can change it in Settings) |
| **Enter** | Opens the selected file |
| **F3** (or **Space** outside the field) | Large preview |
| **↑ ↓ PageUp PageDown** | Move the cursor through the list |
| **Ctrl + Shift + C** | Copies the file path |
| **Ctrl + scroll wheel** | Changes the thumbnail size |
| **Esc** | Closes the preview or hides the small window |

## Find similar files

Right-click a file and choose **Buscar semelhantes** (find similar):

- **Nome parecido** (similar name): `IMG_2048` finds `IMG_2049`, `IMG_2051`...
- **Foto parecida** (similar photo): photos with similar composition and colors (copies, resized versions). It doesn't recognize people or objects.
- **Ícone parecido** (similar icon): files with the same icon.

## FAQ

**Do I need to be an administrator?**
No. But as an administrator on an NTFS disk, the first read takes seconds instead of minutes. Seek shows a hint in the bottom bar and offers **Leitura turbo: reabrir como administrador**.

**Does Seek read the contents of my files?**
No. It reads names, folders, sizes and dates. The only exception is **Achar duplicados**, which compares the content of the files you asked for, only on your computer, and only when you click.

**How does Seek update itself?**
When it opens and every 6 hours, it checks here on GitHub for a new version, downloads it, checks the integrity (SHA-256) and replaces the file. The new version takes effect the next time you open it.

**How do I close the program?**
Closing the window only keeps it in the Windows tray. To close it for real: tray icon, right-click, **Fechar**.

**How do I uninstall it?**
Close Seek from the tray and delete `Seek.exe` and the `indices` folder next to it. If you turned on "Abrir junto com o Windows", switch it off first in Settings.

**How much memory does it use?**
About 400 MB for every 1 million indexed files.

**Does it work on network drives, pen drives, external hard drives?**
Pen drives and external hard drives are included in **Todos os discos**. For a network folder, you pick it as a folder (without turbo reading).

## Limits

- Windows 10 and 11 only, 64-bit. The interface is in Portuguese.
- It doesn't look inside files or recover deleted files.
- PDF and Office thumbnails only appear if Windows has the program that generates them; RAW and HEIC depend on the installed codecs.

## Open source

Seek is free software, under the [MIT license](LICENSE). The source code (Rust) is in this repository.

To build it you need [Rust](https://rustup.rs) (edition 2024) on Windows:

```
cargo build --release
```

The program comes out at `target/release/Seek.exe`. To run the tests: `cargo test`.

Code overview, in `src/`:

| File | What it does |
|---|---|
| `lib.rs` | Engine: index, search, filters, sorting, duplicates, similar files |
| `mft.rs` | Direct read of the NTFS file table (MFT) |
| `watch.rs` | Watches the disk live (file created, deleted, renamed) |
| `ui.rs`, `main.rs` | Interface (egui) and application |
| `tray.rs`, `hotkey.rs` | System tray and global shortcut |
| `icon_search.rs`, `thumbs.rs`, `shellthumb.rs` | Icons, thumbnails and search by similar icon/photo |
| `update.rs` | Automatic update through the GitHub Releases (checks SHA-256) |

Contributions are welcome: open an issue or a pull request.

---

## Versions

See the [published releases](https://github.com/inflzzz/seek/releases) with what's new in each one.

---

<div align="center"><sub>Seek · made in Rust · your computer's files stay on your computer</sub></div>
