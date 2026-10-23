# Grids

A fast, lightweight multi-pane file manager for Windows. Up to 16 Explorer panes in one window, a folder tree on the left, and layouts you change with one drag. One small exe (about 450 KB), nothing to install.

[한국어](README.ko.md)

[![Grids in motion](docs/demo.gif)](docs/demo.mp4)

▶ [Watch the full launch video with sound](docs/demo.mp4) (35 s)

![Grids, light theme](docs/screenshot-light.png)

Every pane is a real Windows Explorer view, so right-click menus, drag and drop, thumbnails, rename and copy progress all work exactly as they do in Explorer.

## Features

- **Free layouts**: up to 4 columns, each with its own number of panes (1 to 4). Click the layout button for an even grid, or press and drag across it to set each column; six presets for common setups. Adding or removing cells keeps every remaining pane in its place, and a pane whose cell comes back returns there.
- **Resizable everything**: drag any boundary between panes or the sidebar (hovering one marks it with a line in the selection colour); double-click a boundary to even it out.
- **Folder tree** that follows the active pane and keeps the current folder's name in view, even in deep folders.
- **Per-pane toolbar**: address bar (led by the folder's icon, as in Explorer), back / forward / up, name filter (`*.pdf;*.txt` or any part of a name; click its icon or press Alt+↓ in it to pick a file type: pictures, videos, music, documents, archives, programs, or the selected file's type; an empty filter shows "Filter names  Ctrl+F"), open the selected folder in another pane (its columns come along, so you can set one pane up and copy it around) or copy / move the selected files to another pane's folder (up to 10,000), swap places with another pane (nothing is reopened: folder, history, selection, filter and columns move along; the save button's dot shows the new arrangement until you save or swap back), favourite toggle; drag a folder onto a pane's top bar to open it there. Screen readers name the fields by pane ("Pane 2 address", "Pane 2 name filter").
- **Sessions**: the layout, folders and each pane's columns (order, widths, sort) are saved automatically. Save named sessions and load them from cards that preview each layout; hover a card for a ✕ that deletes it, or right-click it to rename it in place (a name that's taken is refused, never overwritten); a green dot on the save button shows unsaved layout changes.
- **Missing folders**: a local folder from a session, favourite, recent list or the command line that was deleted or renamed opens its nearest existing parent instead, and the status line says which folder was missing (8 s); it also leaves the recent list.
- **Columns are a pane setting**: their order, widths and sort stay as the pane moves from folder to folder, and any column can be made narrower than Explorer allows (Size stops at 90 px there).
- **Tabs** (View menu → Tabs in panes, off by default): each pane gets a tab strip under its header. Ctrl+T or + opens the current folder in a new tab, Ctrl+W, × or a middle click closes one, Ctrl+PgDn / Ctrl+PgUp or a click switches; double-click the empty strip for a new tab. Tabs are saved with the panes and sessions, and kept while the strips are hidden.
- **Folder favourites**: star a folder from its pane header, jump to it from the star list in the toolbar.
- **Drive buttons**: C: D: E: … in the top toolbar open that drive in the active pane; the active pane's drive is underlined in the selection colour, and buttons come and go as drives are plugged in or removed.
- **Folder sizes**: the status line also shows the total size of selected folders, summed in the background (F5 sums again; "partial" when some items couldn't be read).
- **View menu**: show hidden files and file name extensions — the same switches as Explorer's View > Show.
- **Light and dark themes** with six selection colours each; the active pane is outlined and its header tinted. Can follow Windows' light/dark setting (View menu, on for a new install; the theme button turns it off); switching keeps each pane's history, filter and selection.
- **Pane colour labels**: right-click a pane header to give the pane one of those colours, shown as a thin strip along its top and behind its number, and next to its name in the ⇄ menu; saved with the session.
- **Keyboard focus comes back**: after Alt+Tab, the taskbar, a closed popup or restoring from minimized, the keys go straight to the list, address bar, filter or tree you left, with the text and caret you left in it.
- **Single instance**: running `grids.exe C:\a D:\b` or opening a `.grids` session file reuses the open window.
- **Portable**: settings live in `grids.ini` next to the exe, named sessions in the `sessions` folder beside it. Changes are saved automatically as you make them, so after a Windows restart, sign-out or crash Grids reopens the last state it managed to save.
- **English or Korean** (View menu). The Explorer parts inside each pane, such as column names and right-click menus, follow your Windows display language.

| Layout picker | Sessions | Favourites |
| --- | --- | --- |
| ![Layout picker](docs/layout-picker.png) | ![Sessions](docs/sessions.png) | ![Favourites](docs/favourites.png) |

![Grids, dark theme](docs/screenshot-dark.png)

## Download

Get `grids.exe` from [Releases](https://github.com/efww/grids/releases), put it in any folder and run it.

The exe is not code-signed, so Windows SmartScreen may warn the first time: choose **More info → Run anyway**.

**Requirements**: Windows 10 (1903 or later) or Windows 11, x64.

## Keyboard shortcuts

| Keys | Action |
| --- | --- |
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous pane |
| Ctrl+1 … Ctrl+9 | Go to pane 1–9 (the number at the left of each pane header; hidden in very narrow panes) |
| Alt+Shift+← / → / ↑ / ↓ | Pane to the left / right / above / below |
| Ctrl+Alt+Shift+← / → / ↑ / ↓ | Swap the active pane with the one to the left / right / above / below (its folder, history, selection, filter and columns stay; also in the ⇄ menu) |
| Alt+← / Alt+→ | Back / forward |
| Alt+↑ | Parent folder |
| Alt+↓ | Recent folders (all panes, newest first; right-click ← / → too) |
| Ctrl+L, Alt+D, F4 | Address bar: also `..` or a subfolder name, a file to open, or a command such as `cmd`, `wt`, `powershell` (runs in this pane's folder), or part of a recent or favourite folder's name |
| Ctrl+F | Name filter |
| Alt+↓ (in the filter) | File types menu (or click the filter's icon) |
| F6 / Shift+F6 | Move between file list, address bar, filter and folder tree |
| Enter / Esc | Apply / cancel the address or filter |
| Win+Alt+E | Bring Grids to front from any app, ready to type in the active pane (again: minimize; View menu turns it off) |
| Ctrl+H | Show hidden files |
| F3 | Quick preview of the focused file (arrows: previous / next file without closing it, Esc: close) |
| F7 | New folder (named and ready to rename, as Explorer's Ctrl+Shift+N) |
| Ctrl+T / Ctrl+W | New tab / close tab, with tabs on |
| Ctrl+PgDn / Ctrl+PgUp | Next / previous tab |
| Ctrl+V | With a copied picture (screenshot, browser image): save it here as a PNG file |
| Ctrl+S | Save the current session |
| Ctrl+B | Favourites list (↑/↓, Enter to open, Del to remove, Alt+↑/↓ or drag to reorder) |
| Ctrl+/ | Keyboard shortcuts (this list) |
| Ctrl+Shift+M | Maximize / restore the active pane (or double-click its header) |
| Ctrl+Shift+E | Hide / show the folder tree (also View menu → Folder tree; its width is kept) |
| Ctrl+Shift+O | Open the folders of open Explorer windows and tabs in panes, from the active pane on (also View menu; panes already showing one of them stay put) |
| Ctrl+Shift+C | Copy the full path of the selected items (or of the current folder) |
| Num+ / Ctrl+Shift+A | Select files by pattern (`*.jpg;*.png` or part of a name; Shift+Enter deselects) |
| Num− | Deselect files by pattern |
| Num* | Invert the selection |
| F2, Del, Ctrl+C / X / V, F5 | Same as Explorer |

Double-clicking empty space in a pane goes up a folder (can be turned off in the view menu).

Address bar: click a folder in the path to go there; click a › to list the folders inside; click the current folder or empty space, or press Ctrl+L, to type a path.

## Build

Needs the Rust GNU toolchain (`x86_64-pc-windows-gnu`) and MSYS2's MinGW tools (`windres`, `dlltool`, `as` in `C:\msys64\mingw64\bin`).

```
build.cmd
```

This builds `target\release\grids.exe` and copies it to `grids.exe` in the project folder. `icon.py` redraws the app icon (`app.ico`).

## Credits

Inspired by [Q-Dir](https://www.softwareok.com/?seite=Freeware/Q-Dir). Grids is an independent project, not affiliated with Q-Dir or its author.

## License

[MIT](LICENSE)
