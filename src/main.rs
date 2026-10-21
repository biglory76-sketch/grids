// Grids: multi-pane file manager. Shell tree on the left, up to 4 columns of up to 4 Explorer views on the right.
// Each pane hosts the real Explorer control (IExplorerBrowser), so context menus, drag & drop, rename,
// thumbnails, columns etc. come from Windows itself.
#![cfg_attr(not(test), windows_subsystem = "windows")]

use std::cell::{Cell, RefCell};
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, Once};
use std::time::{Duration, Instant};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMWINDOWATTRIBUTE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW};
use windows::Win32::System::Com::*;
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW, SetClipboardData};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::{MK_CONTROL, MK_SHIFT, MODIFIERKEYS_FLAGS, SFGAO_FLAGS, SFGAO_FOLDER, SFGAO_STREAM};
use windows::Win32::System::Threading::{OpenProcess, INFINITE, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::System::Variant::{VariantClear, VARIANT, VT_I4};
use windows::Win32::UI::Accessibility::{AccessibleObjectFromPoint, IAccessible, ROLE_SYSTEM_LIST};
use windows::Win32::UI::Controls::Dialogs::{ChooseColorW, CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW};
use windows::Win32::UI::Controls::{
    SetWindowTheme, BeginBufferedPaint, BufferedPaintInit, EndBufferedPaint, BPBF_TOPDOWNDIB, DRAWITEMSTRUCT, ODS_SELECTED, TOOLTIPS_CLASSW,
    CDDS_ITEMPREPAINT, CDDS_PREPAINT, CDDS_SUBITEM, CDIS_FOCUS, CDIS_HOT, CDIS_SELECTED, CDRF_NOTIFYITEMDRAW, CDRF_NOTIFYSUBITEMDRAW, CDRF_SKIPDEFAULT,
    LVCDI_GROUP, LVGA_HEADER_CENTER, LVGA_HEADER_RIGHT, LVGF_ALIGN, LVGF_DESCRIPTIONBOTTOM, LVGF_DESCRIPTIONTOP, LVGF_EXTENDEDIMAGE, LVGF_FOOTER,
    LVGF_HEADER, LVGF_STATE, LVGF_SUBTITLE, LVGF_TASK, LVGF_TITLEIMAGE, LVGGR_HEADER, LVGROUP, LVGROUP_MASK, LVGS_COLLAPSED, LVGS_COLLAPSIBLE,
    LVGS_FOCUSED, LVGS_SELECTED, LVGS_SUBSETED, LIST_VIEW_GROUP_STATE_FLAGS, LVM_GETGROUPINFO, LVM_GETGROUPRECT, LVM_GETIMAGELIST, LVSIL_GROUPHEADER,
    HDF_CENTER, HDF_RIGHT, HDF_SORTDOWN, HDF_SORTUP, HDITEMW, HDI_FORMAT, HDI_TEXT, HDM_GETITEMW, HDN_BEGINTRACKW, HDN_ITEMCHANGINGW, HDN_TRACKW, HDI_WIDTH, NMHEADERW, LVCF_MINWIDTH, LVCOLUMNW, LVM_GETCOLUMNW, LVM_SETCOLUMNW,
    LVM_GETCOLUMNWIDTH, LVM_SETCOLUMNWIDTH, NMCUSTOMDRAW, NMHDR, NMLVCUSTOMDRAW, NM_CUSTOMDRAW,
    TTF_IDISHWND, TTF_SUBCLASS, TTM_ADDTOOLW, TTM_UPDATETIPTEXTW, TTTOOLINFOW, WM_MOUSELEAVE,
};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SystemParametersInfoForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::Common::{COMDLG_FILTERSPEC, ITEMIDLIST, STRRET};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;


mod ui_defs;
mod session;
mod geometry;
mod state;
mod fonts;
mod draw;
mod nav;
mod tree;
mod panes;
mod sessions_ui;
mod status;
mod paint;
mod layout_picker;
mod load_popup;
mod favourites;
mod peek;
mod keys_window;
mod paste;
mod select_pattern;
mod keyboard;
mod window_ui;

use ui_defs::*;
// explicit, so it wins over the same name glob-imported from windows (a glob clash would turn match patterns into bindings)
use ui_defs::EN_CHANGE;
use session::*;
use geometry::*;
use state::*;
use fonts::*;
use draw::*;
use nav::*;
use tree::*;
use panes::*;
use sessions_ui::*;
use status::*;
use paint::*;
use layout_picker::*;
use load_popup::*;
use favourites::*;
use peek::*;
use keys_window::*;
use paste::*;
use select_pattern::*;
use keyboard::*;
use window_ui::*;

fn main() -> Result<()> {
    unsafe {
        let args: Vec<String> = std::env::args().skip(1).map(resolve).collect();
        // Single instance: hand the arguments to the running window and leave.
        if let Some(prev) = running_instance() {
            if !args.is_empty() {
                let data: Vec<u16> = args.join("\n").encode_utf16().collect();
                let cd = CopyData { dw: COPY_ID, cb: (data.len() * 2) as u32, lp: data.as_ptr() as _ };
                let lp = LPARAM(&cd as *const CopyData as isize);
                SendMessageTimeoutW(prev, WM_COPYDATA, WPARAM(0), lp, SMTO_ABORTIFHUNG, 5000, None);
            }
            if IsIconic(prev).as_bool() {
                _ = ShowWindow(prev, SW_RESTORE);
            }
            _ = SetForegroundWindow(prev);
            return Ok(());
        }

        OleInitialize(None)?; // OLE (not just COM) for drag & drop
        // A grids.ini that is there but unreadable (locked, no access) must not be overwritten with defaults:
        // retry, or quit before anything could save. The language is not known yet, so both.
        let ini = loop {
            match load_ini(ini_path().parent().unwrap_or(Path::new("."))) {
                Ok(t) => break t,
                Err(e) => {
                    let text = HSTRING::from(format!(
                        "Grids could not read its settings file (grids.ini): {e}. Retry, or Cancel to quit without changing it.\n\n\
                         Grids 설정 파일(grids.ini)을 읽지 못했습니다: {e}. 다시 시도하거나, 취소하면 파일을 바꾸지 않고 종료합니다."
                    ));
                    if MessageBoxW(None, &text, w!("Grids"), MB_RETRYCANCEL | MB_ICONWARNING) != IDRETRY {
                        return Ok(());
                    }
                }
            }
        };
        let mut s = ini.map(|t| Session::parse(&t)).unwrap_or_default();
        match session_arg(&args).and_then(|f| std::fs::read_to_string(f).ok()) {
            Some(t) => s.take_layout(&Session::parse(&t)),
            None => apply_args(&mut s, args),
        }

        _ = BufferedPaintInit();

        let hinst: HINSTANCE = GetModuleHandleW(None)?.into();
        let arrow = LoadCursorW(None, IDC_ARROW)?;
        RegisterClassW(&WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(main_proc),
            hInstance: hinst,
            hCursor: arrow,
            hIcon: LoadIconW(Some(hinst), PCWSTR(1 as _)).unwrap_or_default(), // app.ico, group id 1
            lpszClassName: w!("GridsMain"),
            ..Default::default()
        });
        RegisterClassW(&WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS, // the plate is drawn to the window size
            lpfnWndProc: Some(pane_proc),
            hInstance: hinst,
            hCursor: arrow,
            lpszClassName: w!("GridsPane"),
            ..Default::default()
        });
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(bar_proc),
            hInstance: hinst,
            lpszClassName: w!("GridsBar"),
            ..Default::default()
        });
        RegisterClassW(&WNDCLASSW {
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(fav_proc),
            hInstance: hinst,
            hCursor: arrow,
            lpszClassName: w!("GridsFav"),
            ..Default::default()
        });
        RegisterClassW(&WNDCLASSW {
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(keys_proc),
            hInstance: hinst,
            hCursor: arrow,
            lpszClassName: w!("GridsKeys"),
            ..Default::default()
        });
        RegisterClassW(&WNDCLASSW {
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(load_proc),
            hInstance: hinst,
            hCursor: arrow,
            lpszClassName: w!("GridsLoad"),
            ..Default::default()
        });
        RegisterClassW(&WNDCLASSW {
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(grid_proc),
            hInstance: hinst,
            hCursor: arrow,
            lpszClassName: w!("GridsPicker"),
            ..Default::default()
        });
        RegisterClassW(&WNDCLASSW {
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(match_proc),
            hInstance: hinst,
            hCursor: arrow,
            lpszClassName: w!("GridsMatch"),
            ..Default::default()
        });
        RegisterClassW(&WNDCLASSW {
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(peek_proc),
            hInstance: hinst,
            hCursor: arrow,
            lpszClassName: w!("GridsPeek"),
            ..Default::default()
        });
        KO.store(s.ko, Ordering::Relaxed);
        let view_menu = make_menus()?;
        let main = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("GridsMain"),
            w!("Grids"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1400,
            900,
            None,
            None,
            Some(hinst),
            None,
        )?;
        // caption in the ground colour, so title bar and toolbar read as one surface
        let caption = cref(pal().bg);
        _ = DwmSetWindowAttribute(main, DWMWA_CAPTION_COLOR, &caption as *const _ as _, 4);
        let square = DWMWCP_DONOTROUND;
        _ = DwmSetWindowAttribute(main, DWMWA_WINDOW_CORNER_PREFERENCE, &square as *const _ as _, 4);
        let dpi = GetDpiForWindow(main) as i32;
        if s.side <= 0 {
            s.side = 240 * dpi / 96;
        }
        let win = s.win;
        let tip_style = WS_POPUP | WINDOW_STYLE(0x01 | 0x02); // TTS_ALWAYSTIP | TTS_NOPREFIX
        let tip = CreateWindowExW(WS_EX_TOPMOST, TOOLTIPS_CLASSW, None, tip_style, 0, 0, 0, 0, Some(main), None, None, None)?;
        app(|a| (a.s, a.main, a.dpi, a.view_menu, a.tip) = (s, main, dpi, view_menu, tip));
        let bar_ex = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
        let mut bars = [HWND::default(); 2];
        for b in &mut bars {
            *b = CreateWindowExW(bar_ex, w!("GridsBar"), w!(""), WS_POPUP, 0, 0, 0, 0, Some(main), None, None, None)?;
            _ = SetLayeredWindowAttributes(*b, COLORREF(0), 170, LWA_ALPHA);
        }
        app(|a| a.bars = bars);
        for (id, label, hint) in toolbar_tips() {
            add_tip(child(main, w!("BUTTON"), PCWSTR(HSTRING::from(label).as_ptr()), BS_OWNERDRAW as u32, WINDOW_EX_STYLE(0), id, HFONT::default()), hint);
        }
        if let Ok((t, th)) = create_tree(main) {
            app(|a| (a.tree, a.tree_hwnd) = (Some(t), th));
        }
        apply_theme();
        apply_font();
        // keep the whole window on the work area, whatever screen it was saved on
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        match win {
            Some([l, t, r, b, max]) => {
                let saved = RECT { left: l, top: t, right: r, bottom: b };
                let mut fixed = saved;
                if GetMonitorInfoW(MonitorFromRect(&saved, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool() {
                    fixed = fit_saved(saved, mi.rcMonitor, mi.rcWork);
                    // still hidden: move onto the target monitor first, so a DPI change happens here and not after
                    // the placement below (WM_DPICHANGED would apply Windows' scaled size on top of it)
                    let (dx, dy) = (mi.rcWork.left - mi.rcMonitor.left, mi.rcWork.top - mi.rcMonitor.top);
                    let (w, h) = (fixed.right - fixed.left, fixed.bottom - fixed.top);
                    _ = SetWindowPos(main, None, fixed.left + dx, fixed.top + dy, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
                }
                let wp = WINDOWPLACEMENT {
                    length: size_of::<WINDOWPLACEMENT>() as u32,
                    showCmd: if max != 0 { SW_SHOWMAXIMIZED.0 as u32 } else { SW_SHOWNORMAL.0 as u32 },
                    rcNormalPosition: fixed,
                    ..Default::default()
                };
                _ = SetWindowPlacement(main, &wp);
            }
            None => {
                let mut r = RECT::default();
                _ = GetWindowRect(main, &mut r);
                let ok = GetMonitorInfoW(MonitorFromWindow(main, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool();
                let f = fit(r, mi.rcWork);
                if ok && f != r {
                    _ = SetWindowPos(main, None, f.left, f.top, f.right - f.left, f.bottom - f.top, SWP_NOZORDER | SWP_NOACTIVATE);
                }
                _ = ShowWindow(main, SW_SHOWNORMAL);
            }
        }
        relayout();
        focus_view(app(|a| a.s.active));
        let file = app(|a| a.s.file.clone());
        let base = std::fs::read_to_string(&file).ok().map(|t| Session::parse(&t));
        app(|a| a.base = Some(base.unwrap_or_else(|| a.s.clone())));
        refresh_save_button();
        SetTimer(Some(main), TIMER_STATUS, 300, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if !pre_translate(&msg) {
                let up = up_candidate(&msg);
                _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
                if let Some((i, g, svh)) = up {
                    let query = || {
                        let view = view_of::<IShellView>(i).and_then(|v| v.GetWindow().ok()).map(|h| h.0 as isize);
                        let sel = view_of::<IFolderView2>(i).and_then(|fv| fv.ItemCount(SVGIO_SELECTION).ok()).map(|n| n as u32);
                        (view, sel)
                    };
                    if confirm((g, svh), query, gen) {
                        browse(i, SBSP_PARENT);
                    }
                }
            }
        }
        // The window is gone; copies still running finish first. Waiting keeps COM calls and messages
        // moving, in case a shell extension calls back into this apartment.
        for job in JOBS.take() {
            if !job.is_finished() {
                let h = HANDLE(std::os::windows::io::AsRawHandle::as_raw_handle(&job));
                _ = CoWaitForMultipleHandles((COWAIT_DISPATCH_CALLS | COWAIT_DISPATCH_WINDOW_MESSAGES).0 as u32, INFINITE, &[h]);
            }
            _ = job.join();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbours() {
        let r = |left, top, right, bottom| RECT { left, top, right, bottom };
        // 2×2 with an 8px gap: slots run down each column
        let grid = [(0, r(0, 0, 100, 100)), (1, r(0, 108, 100, 208)), (2, r(108, 0, 208, 100)), (3, r(108, 108, 208, 208))];
        let n = |cells: &[(usize, RECT)], cur, dx, dy| neighbour(cells, cur, dx, dy);
        assert_eq!([n(&grid, 3, 0, -1), n(&grid, 3, -1, 0), n(&grid, 0, 1, 0), n(&grid, 0, 0, 1)], [Some(2), Some(1), Some(2), Some(1)]);
        // edges
        assert_eq!([n(&grid, 0, -1, 0), n(&grid, 0, 0, -1), n(&grid, 3, 1, 0), n(&grid, 3, 0, 1)], [None; 4]);
        // [1,3]: one tall cell on the left, three on the right
        let tall = [(0, r(0, 0, 100, 316)), (1, r(108, 0, 208, 100)), (2, r(108, 108, 208, 208)), (3, r(108, 216, 208, 316))];
        assert_eq!([n(&tall, 0, 1, 0), n(&tall, 3, -1, 0), n(&tall, 2, 0, -1), n(&tall, 2, 0, 1)], [Some(2), Some(0), Some(1), Some(3)]);
        assert_eq!(n(&tall, 1, 0, -1), None); // stays in its column
        // a pane that failed to open: neither it nor the diagonal one is picked
        let gone = [grid[0], grid[2], grid[3]];
        assert_eq!([n(&gone, 0, 0, 1), n(&gone, 3, -1, 0), n(&gone, 2, 0, 1)], [None, None, Some(3)]);
        assert_eq!(n(&grid, 7, 1, 0), None);
    }

    #[test]
    fn address_crumbs() {
        let c = |p: &str| crumbs(p).into_iter().map(|(n, g)| format!("{n}={g}")).collect::<Vec<_>>();
        assert_eq!(c("C:\\"), [r"C:=C:\"]);
        assert_eq!(c(r"C:\Users\Public\"), [r"C:=C:\", r"Users=C:\Users", r"Public=C:\Users\Public"]);
        assert_eq!(c(r"\\srv\share\a"), [r"\\srv\share=\\srv\share", r"a=\\srv\share\a"]);
        assert!(crumbs(THIS_PC).is_empty());
        let p = r"D:\x\y y\z";
        assert!(crumbs(p).iter().all(|(_, g)| p.starts_with(g.as_str())));
    }

    #[test]
    fn crumb_fits() {
        let f = |w: &[i32], avail| crumb_fit(w, avail, 4, 20, 16);
        assert_eq!(f(&[30, 40, 50], 500), (0, false, 50));
        assert_eq!(f(&[14, 15, 15], 52), (0, false, 15)); // exactly fits: nothing folded
        assert_eq!(f(&[14, 15, 15], 51), (2, true, 15));
        assert_eq!(f(&[10, 200], 100), (1, false, 100)); // current folder longer than the field
        assert_eq!(f(&[30, 10, 50], 70), (2, false, 50)); // no room left for '…'
        assert_eq!(f(&[30, 10, 50], 78), (2, true, 50));
        assert_eq!(f(&[30, 40], 15), (2, false, 0));
        assert_eq!(f(&[30, 40], 0), (2, false, 0));
        assert_eq!(f(&[], 100), (0, false, 0));
    }

    #[test]
    fn focus_rings() {
        let walk = |back, tree| (0..4).map(|p| focus_ring(p, back, tree)).collect::<Vec<_>>();
        assert_eq!(walk(false, true), [1, 2, 3, 0]);
        assert_eq!(walk(true, true), [3, 0, 1, 2]);
        assert_eq!(walk(false, false)[..3], [1, 2, 0]);
        assert_eq!(walk(true, false)[..3], [2, 0, 1]);
    }

    #[test]
    fn hidden_tree_keeps_its_width() {
        // hidden, narrow window, column drag: the clamped 536 must not replace 600
        assert_eq!(kept_side(false, 600, 536), 600);
        assert_eq!(kept_side(true, 600, 536), 536); // shown: as before
        let s = Session::parse(&Session { tree: false, side: 600, ..Session::default() }.to_ini());
        assert_eq!((s.tree, s.side), (false, 600));
        assert!(Session::parse("side=300\ndblup=1\n").tree); // older ini: shown
        // grid starting at pad: the old sidebar splitter is no longer a splitter
        let g = Geo { w: 1000, h: 600, top: 34, bot: 576, pad: 8, gap: 8, side: 240, tree: false, gx: 8, xs: vec![(8, 992)], ys: vec![vec![(34, 576)]], zoom: false };
        assert!((240..264).all(|x| !matches!(hit(&g, x, 100), Some(Hit::Side))));
    }

    #[test]
    fn tree_input_kept_only_for_tree_keys() {
        assert!(tree_input_after(false, true, false)); // arrow in the tree
        assert!(!tree_input_after(true, true, true)); // Ctrl+Tab / F6 in the tree
        assert!(tree_input_after(true, false, false) && !tree_input_after(false, false, false)); // elsewhere: unchanged
    }

    #[test]
    fn columns() {
        let k = |pid| PROPERTYKEY { fmtid: GUID::from_u128(0xB725F130_47EF_101A_A5F1_02608C9EEBAC), pid };
        let text = cols_text(&[(k(10), 220), (k(12), 80)], &[(k(12), -1)]);
        assert_eq!(parse_cols(&text), (vec![(k(10), 220), (k(12), 80)], vec![(k(12), -1)]));
        assert_eq!(parse_cols("junk;"), (vec![], vec![]));
        // kept in the session; a change counts wherever the pane is (columns are a pane setting)
        let mut base = Session { paths: vec![r"C:\a".into(); MAXP], ..Default::default() };
        base.cols[0] = text.clone();
        let base = Session::parse(&base.to_ini());
        assert_eq!(base.cols[0], text);
        let mut cur = base.clone();
        cur.cols[0] = cols_text(&[(k(12), 80), (k(10), 220)], &[(k(12), -1)]);
        assert!(layout_differs(&cur, &base));
        cur.paths[0] = r"C:\b".into();
        assert!(layout_differs(&cur, &base));
        assert!(is_fs(r"C:\") && is_fs(r"\\srv\share") && !is_fs("::{20D04FE0-3AEA-1069-A2D8-08002B30309D}"));
        let titles = [r"C:\Users\x\Downloads", "", r"C:\"].map(window_title);
        assert_eq!(titles, ["Downloads - Grids", "Grids", "C: - Grids"]);
    }

    #[test]
    fn dblclick_up() {
        let c = |time, x, y| Click { win: 7, gen: 1, time, x, y };
        let next = |prev, now, bg| next_click(prev, now, bg, 500, 4, 4);
        // background, then background in time and box: up, and the pair is consumed
        let (rec, up) = next(None, c(100, 10, 10), true);
        assert!(rec == Some(c(100, 10, 10)) && !up);
        let (rec, up) = next(rec, c(400, 12, 8), true);
        assert!(rec.is_none() && up);
        assert!(!next(rec, c(450, 12, 8), true).1); // third click
        // first press on an item: the next one only records
        let (rec, up) = next(None, c(100, 10, 10), false);
        assert!(rec.is_none() && !up);
        assert!(!next(rec, c(200, 10, 10), true).1);
        // out of time, x, y
        let p = Some(c(100, 10, 10));
        assert!(!next(p, c(601, 10, 10), true).1);
        assert!(!next(p, c(200, 13, 10), true).1);
        assert!(!next(p, c(200, 10, 7), true).1);
        // MSG.time wraps
        assert!(next(Some(c(u32::MAX - 10, 10, 10)), c(20, 10, 10), true).1);
        // a navigation between the clicks
        assert!(!next(p, Click { gen: 2, ..c(200, 10, 10) }, true).1);

        assert!(bg_role(33));
        assert!([34, 20, 43, 25, 3].iter().all(|&r| !bg_role(r)));

        // a navigation during the probe voids it; g0 is the value before
        let g = Cell::new(5);
        assert_eq!(bg_probe(|| g.get(), || { g.set(6); true }), (5, false));
        assert_eq!(bg_probe(|| g.get(), || true), (6, true));

        // the generation is read after the queries (sol regression)
        let g = Cell::new(5);
        assert!(!confirm((5, 9), || { g.set(6); (Some(9), Some(0)) }, || g.get()));
        let g = || 5;
        assert!(!confirm((5, 9), || (Some(8), Some(0)), g));
        assert!(!confirm((5, 9), || (Some(9), Some(1)), g));
        assert!(!confirm((5, 9), || (Some(9), None), g));
        assert!(!confirm((5, 9), || (None, Some(0)), g));
        assert!(confirm((5, 9), || (Some(9), Some(0)), g));
    }

    #[test]
    fn addr() {
        let x = r"C:\Program Files\x.exe";
        assert_eq!(addr_parts("cmd"), Some((Some("cmd"), Some(("cmd", "")))));
        assert_eq!(addr_parts("powershell -NoExit").unwrap().1, Some(("powershell", "-NoExit")));
        assert_eq!(addr_parts(r#""C:\Program Files\x.exe" -a"#), Some((None, Some((x, "-a")))));
        assert_eq!(addr_parts(r#""C:\Program Files\x.exe""#), Some((Some(x), Some((x, "")))));
        assert_eq!(addr_parts(r#""C:\Windows\notepad.exe"#), None);
        assert_eq!(addr_parts(r"C:\Program Files\x.exe -a").unwrap().1, None);
        assert_eq!(addr_parts(r#"cmd /k "echo hi""#), Some((None, Some(("cmd", r#"/k "echo hi""#)))));
        assert_eq!(addr_parts(r#""a"b"#), Some((None, None)));
        assert!(is_rel("..") && is_rel(r"sub\x"));
        assert!(!is_rel(r"C:\") && !is_rel(r"\\srv\s") && !is_rel("shell:Downloads") && !is_rel("::{GUID}"));
    }

    #[test]
    fn paste() {
        let v = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(paste_mode(&v(&[r"C:\a.txt"]), r"C:\"), Some(true));
        assert_eq!(paste_mode(&v(&[r"c:\X\a.txt", r"C:\x\b\"]), r"C:\X\"), Some(true));
        assert_eq!(paste_mode(&v(&[r"C:\a.txt"]), r"D:\x"), Some(false));
        assert_eq!(paste_mode(&v(&[r"\\srv\share\a.txt"]), r"\\SRV\share"), Some(true));
        assert_eq!(paste_mode(&v(&[r"\\srv\share\a.txt"]), r"C:\x"), Some(false));
        assert_eq!(paste_mode(&v(&[r"C:\a.txt", r"D:\b.txt"]), r"C:\"), None); // mixed
        assert_eq!(paste_mode(&[], r"C:\"), None);
        assert_eq!(paste_mode(&v(&["::{x}", r"C:\a"]), r"D:\"), None);
        assert_eq!(paste_mode(&v(&[r"C:\"]), r"D:\"), None); // a drive itself has no parent

        // (tries, shell): first press starts one copy, no shell; a held key's repeat: neither; unsupported: shell
        let run = |repeat: bool, ok: bool| {
            let mut tries = 0;
            let handled = paste_key(repeat, || {
                tries += 1;
                ok
            });
            (tries, !handled)
        };
        assert_eq!(run(false, true), (1, false));
        assert_eq!(run(true, true), (0, false));
        assert_eq!(run(false, false), (1, true));

        // ⇄ copy / move: a move into the same folder and mixed paths are refused
        assert_eq!(send_mode(&v(&[r"C:\x\a.txt"]), r"C:\x", false), Some(true));
        assert_eq!(send_mode(&v(&[r"C:\x\a.txt"]), r"C:\x", true), None);
        assert_eq!(send_mode(&v(&[r"C:\x\a.txt"]), r"D:\y", true), Some(false));
        assert_eq!(send_mode(&v(&[r"C:\x\a.txt", r"D:\y\b.txt"]), r"D:\y", false), None);

        // the whole selection or nothing
        let mut calls = 0;
        let all = collect_all(10_000, |k| {
            calls += 1;
            Some(k.to_string())
        });
        assert_eq!((all.map(|v| v.len()), calls), (Ok(10_000), 10_000));
        let mut calls = 0;
        let many = collect_all(10_001, |k| {
            calls += 1;
            Some(k.to_string())
        });
        assert_eq!((many, calls), (Err(Pick::TooMany), 0));
        assert_eq!(collect_all(0, |_| Some(String::new())), Err(Pick::Empty));
        assert_eq!(collect_all(5, |k| (k != 2).then(|| k.to_string())), Err(Pick::Unreadable));
    }

    #[test]
    fn session_layout_args() {
        let mut s = Session { side: 200, active: 3, ..Default::default() };
        s.set_heights(vec![1, 3]);
        (s.colr, s.rowr[1]) = (vec![0.25, 0.75], vec![0.5, 0.25, 0.25]);
        s.win = Some([1, 2, 300, 400, 1]);
        s.paths[1] = r"C:\a=b\폴더".into();
        s.views[1] = (4, 16);
        assert_eq!(Session::parse(&s.to_ini()), s);
        // garbage in the ini falls back to sane values
        let bad = Session::parse("cols=9,0,2\ncolr=1,-1\nrowr=x|1|1,1\nactive=99\nv3=x\np99=C:\\");
        assert_eq!((bad.heights.clone(), bad.colr.clone(), bad.active), (vec![4, 1, 2], even(3), 0));
        assert_eq!(bad.rowr, vec![even(4), vec![1.0], even(2)]);
        // an older even-grid ini still loads: rows x cols, one row-ratio group for every column
        let old = Session::parse("grid=3,2\nrowr=0.5,0.25,0.25");
        assert_eq!((old.heights.clone(), old.rowr[1].clone()), (vec![3, 3], vec![0.5, 0.25, 0.25]));
        // a named session brings grid + folders but not the window
        let mut cur = Session { side: 300, win: Some([0, 0, 9, 9, 0]), ..Default::default() };
        cur.take_layout(&s);
        assert_eq!((cur.heights.clone(), cur.paths[1].as_str(), cur.side, cur.win), (vec![1, 3], s.paths[1].as_str(), 300, Some([0, 0, 9, 9, 0])));

        // panes are numbered down each column; the picker drag sets one column at a time
        assert_eq!([slot(&[1, 3], 0), slot(&[1, 3], 1), slot(&[1, 3], 3)], [(0, 0), (1, 0), (1, 2)]);
        assert_eq!(drag_heights(drag_heights(vec![1], 0, 1), 2, 1), vec![1, 3]); // left 1, then right column down to row 3
        assert_eq!(drag_heights(vec![2, 2, 2], 1, 0), vec![2]); // moving back left drops columns
        assert_eq!((layout_label(&[2, 2]), layout_label(&[1, 3])), ("2 × 2".into(), "1 | 3".into()));

        // green dot: layout, ratios and view modes count; folders and unknown (0) baseline views don't
        let base = s.clone();
        let mut cur = s.clone();
        (cur.paths[0], cur.views[2]) = ("D:\\".into(), (1, 48));
        assert!(!layout_differs(&cur, &base));
        cur.colr = vec![0.3, 0.7];
        assert!(layout_differs(&cur, &base));
        assert!(!s.snapshot().contains("win=") && !s.snapshot().contains("session="));
        // colour labels: round trip, garbage and older ini -> none; a label change alone counts; named sessions keep them
        let mut t = s.clone();
        t.tints[3] = 4;
        assert_eq!(Session::parse(&t.to_ini()).tints[3], 4);
        assert!(t.snapshot().contains("t3=4\n") && !s.to_ini().contains("t3="));
        assert_eq!(Session::parse("t5=9\nt6=x\nt99=2").tints, vec![0; MAXP]);
        assert_eq!(Session::parse(&s.to_ini()).tints, vec![0; MAXP]);
        assert!(layout_differs(&t, &s));
        // the label strip sits inside the border and above the buttons and fields
        for dpi in [96, 120, 144, 192] {
            for w in [120, 400] {
                for active in [false, true] {
                    let (b, top) = (tint_band(w, dpi, active), pane_geo(w, 300, dpi).num.top);
                    let border = if active { 2 * dpi / 96 } else { 1 };
                    assert!(b.top >= border && b.bottom <= top && b.bottom - b.top >= 2, "{dpi} {w} {active}");
                }
            }
        }
        // the drawn star: solid middle and empty corners when filled; hollow middle as an outline
        let (full, ring) = (star_coverage(15, Star::Filled), star_coverage(15, Star::Outline));
        assert_eq!((full[7 * 15 + 7], full[0], full[14 * 15 + 14]), (255, 0, 0));
        assert!(ring[8 * 15 + 7] == 0 && ring.iter().any(|&a| a == 255));
        for n in [15, 22] {
            // the + (what Plus adds to the outline) sits exactly on the box centre in both directions
            let (plus, ring) = (star_coverage(n, Star::Plus), star_coverage(n, Star::Outline));
            let added: Vec<(i32, i32)> = (0..n * n).filter(|&k| plus[k as usize] != ring[k as usize]).map(|k| (k % n, k / n)).collect();
            let span = |f: fn(&(i32, i32)) -> i32| added.iter().map(f).min().unwrap() + added.iter().map(f).max().unwrap();
            assert_eq!((span(|p| p.0), span(|p| p.1)), (n - 1, n - 1), "{n}");
        }
        // favourites: kept in grids.ini, case-insensitive, never in a named session
        let mut f = Session { favs: vec![r"C:\Work".into(), r"D:\".into()], ..Default::default() };
        f = Session::parse(&f.to_ini());
        assert!(f.is_fav(r"c:\work") && !f.is_fav(r"C:\Work\x") && !f.is_fav(""));
        assert!(!f.snapshot().contains("fav="));
        assert_eq!((short_name(r"C:\Users\x\"), short_name("C:\\"), short_name(THIS_PC)), ("x".into(), "C:".into(), "This PC".into()));

        // cells tile the length exactly, with `gap` between neighbours
        let c = cells(10, 305, &[0.25, 0.25, 0.5], 5);
        assert_eq!((c[0].0, c[2].1), (10, 315));
        assert!(c.windows(2).all(|w| w[0].1 + 5 == w[1].0));

        // dragging respects the minimum size and keeps the pair's total
        let mut r = vec![0.5, 0.5];
        drag_ratio(&mut r, 0, -1000, 0, 205, 5, 20);
        assert_eq!(r[0], 0.1);
        assert!((r[0] + r[1] - 1.0).abs() < 1e-12);

        let mut s = Session::default();
        apply_args(&mut s, (0..5).map(|i| format!("::{{x{i}}}")).collect());
        assert_eq!((s.heights.clone(), s.paths[4].as_str()), (vec![2, 2, 2], "::{x4}"));
        assert_eq!((session_arg(&["a.GRIDS".into()]), session_arg(&["old.qdr".into()])), (Some("a.GRIDS"), Some("old.qdr")));
        assert_eq!(session_arg(&["a.grids".into(), "b".into()]), None);

        assert_eq!(filter_spec(" pdf; *.TXT ;"), "*pdf*;*.TXT");
        assert_eq!((human(10), human(1536), human(5 << 30)), ("10 B".into(), "1.5 KB".into(), "5.0 GB".into()));

        // pane top bar: number, back, fwd, up, address, ⇄, filter left to right, nothing overlapping, inside the plate
        for (w, dpi) in [(800, 96), (260, 96), (1200, 144)] {
            let p = pane_geo(w, 500, dpi);
            let row = [p.num, p.btn[0], p.btn[1], p.btn[2], p.addr, p.btn[3], p.filt, p.fav];
            assert!(row.windows(2).all(|x| x[0].right <= x[1].left), "{w}@{dpi}");
            assert!(p.num.right > p.num.left && p.fav.right <= w && p.view.top >= p.btn[0].bottom);
        }
        // the number only shows while the address edit keeps s(32); otherwise the bar is laid out as before
        for dpi in [96, 144] {
            let s = |v: i32| v * dpi / 96;
            for w in 60..=600 {
                let p = pane_geo(w, 500, dpi);
                if p.num.right > p.num.left {
                    let row = [p.num, p.btn[0], p.btn[1], p.btn[2], p.addr, p.btn[3], p.filt, p.fav];
                    assert!(row.windows(2).all(|x| x[0].right <= x[1].left), "{w}@{dpi}");
                    assert!((p.addr.right - s(6)) - (p.addr.left + s(8)) >= s(32), "{w}@{dpi}");
                } else {
                    assert!(p.btn[0].left == s(4) && p.addr.left == s(4) + 3 * s(24) + s(4), "{w}@{dpi}");
                }
            }
        }
        let shown = |w, dpi| {
            let p = pane_geo(w, 500, dpi);
            p.num.right > p.num.left
        };
        assert!(!shown(240, 96) && shown(241, 96) && !shown(361, 144) && shown(362, 144));
    }

    #[test]
    fn missing_folders() {
        assert!(gone(HRESULT::from_win32(2)) && gone(HRESULT::from_win32(3)));
        for c in [E_ACCESSDENIED, HRESULT(0x80070035u32 as i32), HRESULT(0x80070015u32 as i32)] {
            assert!(!gone(c)); // denied, bad network path, not ready
        }
        use std::io::{Error as E, ErrorKind as K};
        let only_a = |p: &Path| if p == Path::new(r"C:\a") { Ok(true) } else { Err(E::from(K::NotFound)) };
        assert_eq!(near_dir(r"C:\a\b\c", only_a).as_deref(), Some(r"C:\a"));
        let denied = |p: &Path| if p == Path::new(r"C:\a\b") { Err(E::from(K::PermissionDenied)) } else { Ok(true) };
        assert_eq!(near_dir(r"C:\a\b\c", denied), None);
        assert_eq!(near_dir(r"C:\a\b\c", |_| Err(E::from(K::NotFound))), None);
        assert_eq!(near_dir(r"C:\a\b\c", |_| Ok(false)), None); // a file in the way

        let (m, t) = (r"C:\a\b".to_string(), r"C:\a".to_string());
        let start = || Some((m.clone(), t.clone()));
        let mut r = start(); // first navigation, synchronous
        assert_eq!(rec_step(&mut r, Nav::Pending, &t), None);
        assert_eq!(rec_step(&mut r, Nav::Complete, r"c:\A"), Some(m.clone())); // case only differs
        assert!(r.is_none());
        let mut r = start(); // another navigation took over
        rec_step(&mut r, Nav::Pending, &t);
        rec_step(&mut r, Nav::Pending, r"C:\x");
        assert!(r.is_none() && rec_step(&mut r, Nav::Complete, r"C:\x").is_none());
        let mut r = start(); // an earlier navigation's failure
        rec_step(&mut r, Nav::Failed, r"C:\y");
        assert_eq!(rec_step(&mut r, Nav::Complete, &t), Some(m.clone()));
        let mut r = start(); // the parent itself failed
        rec_step(&mut r, Nav::Failed, &t);
        assert!(r.is_none());
        let mut r = start(); // some other folder completed
        assert!(rec_step(&mut r, Nav::Complete, r"C:\x").is_none() && r.is_none());
    }

    #[test]
    fn jump_picks() {
        let v = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let recent = v(&[r"C:\Users\me\Downloads", r"C:\Work\down", r"D:\Old Downloads", r"D:\문서\보고서 2026", THIS_PC]);
        let favs = v(&[r"E:\Down", r"\\srv\share", r"C:\"]);
        let pick = |q: &str, cur: &str| jump_pick(q, &recent, &favs, cur);
        // exact beats prefix beats inside; ties go to recent before favourites, newer before older
        assert_eq!(pick("down", ""), Some(r"C:\Work\down"));
        assert_eq!(pick("DOWNL", ""), Some(r"C:\Users\me\Downloads"));
        assert_eq!(pick("loads", ""), Some(r"C:\Users\me\Downloads"));
        assert_eq!(pick("  down  ", r"c:\work\DOWN"), Some(r"E:\Down")); // the folder on display is skipped
        assert_eq!(pick("보고", ""), Some(r"D:\문서\보고서 2026"));
        assert_eq!(pick("share", ""), Some(r"\\srv\share"));
        assert_eq!(pick("c", ""), Some(r"C:\")); // a drive root is named C:
        assert_eq!(pick("this", ""), None); // shell places aren't candidates
        for q in ["", "   ", r"down\x", "a/b", "c:", "\"down\"", "nothing"] {
            assert_eq!(pick(q, ""), None, "{q:?}");
        }
        // only text that named no path falls to a jump; a resolved path that won't open just beeps
        assert!(addr_jumps(None, false));
        assert!(!addr_jumps(None, true));
        assert!(!addr_jumps(Some(true), false) && !addr_jumps(Some(false), false));
    }

    #[test]
    fn recent_visits() {
        let v = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let mut r = Vec::new();
        visit(&mut r, r"C:\a", r"C:\b");
        assert_eq!(r, v(&[r"C:\b", r"C:\a"]));
        visit(&mut r, r"C:\b", r"c:\A"); // case-insensitive: moves up, no duplicate
        assert_eq!(r, v(&[r"c:\A", r"C:\b"]));
        let before = r.clone();
        for (old, new) in [(r"C:\b", r"c:\B"), ("", r"C:\x"), (r"C:\b", THIS_PC)] {
            visit(&mut r, old, new); // refresh / restore, fresh pane, shell place: no visit
        }
        assert_eq!(r, before);
        visit(&mut r, THIS_PC, r"D:\"); // leaving a shell place records only the folder
        assert_eq!(r[0], r"D:\");
        assert!(!r.iter().any(|p| p == THIS_PC));
        for k in 0..30 {
            visit(&mut r, r"C:\b", &format!(r"C:\n{k}"));
        }
        assert_eq!((r.len(), r[0].as_str(), r[1].as_str()), (RECENT, r"C:\n29", r"C:\b"));
    }

    #[test]
    fn recent_survives_restore() {
        let full: Vec<String> = (0..RECENT).map(|k| format!(r"C:\f{k}")).collect();
        let mut r = full.clone();
        // 16 panes come back to their folders, twice (start-up, then a theme / language rebuild)
        for _ in 0..2 {
            for p in &full[..16] {
                visit(&mut r, p, p);
            }
        }
        assert_eq!(r, full);
        let items = recent_items(&r, r"c:\F0");
        assert_eq!(items.len(), RECENT - 1);
        assert_eq!(items.last(), full.last()); // the oldest is still offered
    }

    #[test]
    fn recent_ini() {
        let mut s = Session { recent: vec![r"C:\b".into(), r"\\srv\share".into(), r"C:\a".into()], ..Default::default() };
        s = Session::parse(&s.to_ini());
        assert_eq!(s.recent, [r"C:\b", r"\\srv\share", r"C:\a"]);
        assert!(!s.snapshot().contains("recent="));
        let many: String = (0..30).map(|k| format!("recent=C:\\{k}\n")).collect();
        assert_eq!(Session::parse(&many).recent.len(), RECENT);
    }

    #[test]
    fn ini_write_and_load() {
        let dir = tree("ini", &[]);
        let ini = dir.join("grids.ini");
        let tmp = dir.join("grids.ini.tmp");
        // nothing there: defaults; only qdir.ini: that one
        assert!(load_ini(&dir).unwrap().is_none());
        std::fs::write(dir.join("qdir.ini"), "dark=1\n").unwrap();
        assert_eq!(load_ini(&dir).unwrap().as_deref(), Some("dark=1\n"));
        // the second write wins, round-trips, and leaves no .tmp
        let s = Session { recent: vec![r"C:\a".into()], ..Default::default() };
        write_ini(&ini, "dark=0\n").unwrap();
        write_ini(&ini, &s.to_ini()).unwrap();
        let back = load_ini(&dir).unwrap().unwrap();
        assert_eq!(back, s.to_ini());
        assert_eq!(Session::parse(&back).to_ini(), s.to_ini());
        assert!(!tmp.exists());
        // a grids.ini that cannot be read is an error, not a fall back to qdir.ini / defaults
        std::fs::remove_file(&ini).unwrap();
        std::fs::create_dir(&ini).unwrap();
        assert!(load_ini(&dir).is_err());
        // and a swap that fails reports it and cleans up
        assert!(write_ini(&ini, "dark=1\n").is_err());
        assert!(!tmp.exists() && ini.is_dir());
        _ = std::fs::remove_dir_all(&dir);
    }

    // A fresh folder under %TEMP% holding `files` (relative path, size).
    fn tree(name: &str, files: &[(String, usize)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("grids-test-{}-{name}", std::process::id()));
        _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for (f, n) in files {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, vec![0u8; *n]).unwrap();
        }
        root
    }

    fn many(prefix: &str, count: usize, size: usize) -> Vec<(String, usize)> {
        (0..count).map(|k| (format!("{prefix}{k}.bin"), size)).collect()
    }

    #[test]
    fn folder_size_walk() {
        use std::cell::Cell;
        let never = || false;
        let mut quiet = |_, _| {};
        let root = tree("walk", &[("a.bin".into(), 100), (r"sub\b.bin".into(), 20), (r"sub\deep\c.bin".into(), 3)]);
        assert_eq!(dir_bytes(&root, &never, &never, &mut quiet), Some((123, false)));
        assert_eq!(dir_bytes(&root.join("missing"), &never, &never, &mut quiet), Some((0, true)));
        // a junction pointing back up is not followed
        let made = std::process::Command::new("cmd").args(["/c", "mklink", "/J"]).arg(root.join(r"sub\loop")).arg(&root).output();
        assert!(made.is_ok_and(|o| o.status.success()) && root.join(r"sub\loop\a.bin").exists());
        assert_eq!(dir_bytes(&root, &never, &never, &mut quiet), Some((123, false)));
        // a stop mid-walk gives no total
        let calls = Cell::new(0);
        let later = || {
            calls.set(calls.get() + 1);
            calls.get() > 2
        };
        assert_eq!(dir_bytes(&root, &later, &never, &mut quiet), None);

        // many files, no subfolders: a copy that starts and ends between status ticks only pauses the walk
        let flat = tree("flat", &many("f", 300, 2));
        let seen = Cell::new(0);
        let copying = || {
            seen.set(seen.get() + 1);
            (100..103).contains(&seen.get())
        };
        assert_eq!(dir_bytes(&flat, &never, &copying, &mut quiet), Some((600, false)));
        assert!(seen.get() > 300);
        // a cancel still gets through while paused
        let paused = Cell::new(0);
        let copy_on = || {
            paused.set(paused.get() + 1);
            paused.get() > 50
        };
        let cancel = || paused.get() > 52;
        assert_eq!(dir_bytes(&flat, &cancel, &copy_on, &mut quiet), None);
        assert!(paused.get() < 60);
        // progress comes with the running total
        let big = tree("big", &many("g", 2100, 1));
        let mut last = 0;
        let mut progress = |b, _| last = b;
        assert_eq!(dir_bytes(&big, &never, &never, &mut progress), Some((2100, false)));
        assert!(last >= 2000);

        // F5 after a file was added: the same selection is summed again
        let mut job = SizeJob::default();
        let target = vec![flat.to_string_lossy().into_owned()];
        assert!(job.want(&target, false));
        assert!(job.publish(job.gen, 600, true, false));
        std::fs::write(flat.join("new.bin"), [0u8; 50]).unwrap();
        assert!(!job.want(&target, false));
        job.want(&[], true); // what resum_sizes does
        assert!(job.want(&target, false) && !job.done);
        assert_eq!(dir_bytes(&flat, &never, &never, &mut quiet), Some((650, false)));
        for r in [root, flat, big] {
            _ = std::fs::remove_dir_all(r);
        }
    }

    #[test]
    fn folder_size_job() {
        let (a, b) = (vec![r"C:\a".to_string()], vec![r"C:\b".to_string()]);
        let mut j = SizeJob::default();
        assert!(j.want(&a, false));
        let g = j.gen;
        assert!(!j.want(&a, false)); // same target: keeps running
        assert!(j.publish(g, 10, true, false) && j.done);
        assert!(!j.want(&a, false) && j.done && j.bytes == 10); // and keeps its result
        assert!(!j.want(&a, true)); // a copy starts: cancelled
        assert!(!j.publish(g, 99, true, false) && j.bytes == 10); // the old walk's result is dropped
        assert!(j.want(&a, false) && j.bytes == 0 && !j.done); // the copy ended: summed again
        assert!(!j.want(&[], false) && j.target.is_empty()); // no folders: cancelled
        assert!(j.want(&a, false));
        let g = j.gen;
        assert!(j.want(&b, false) && j.gen != g);
        assert!(!j.publish(g, 5, false, false));
        assert!(j.publish(j.gen, 5, false, true) && j.partial);
    }

    #[test]
    fn peek_newest_only() {
        let req = |p: &str| Some(PeekReq { path: p.into(), w: 10, h: 10, hwnd: 0 });
        let pix = || Some(Pix { w: 1, h: 1, bgra: vec![0; 4], size: Some(3) });
        let mut j = PeekJob { gen: 0, req: None, done: None };
        j.want(req("a"));
        let a = j.gen;
        j.want(req("b")); // arrow pressed before a's picture came
        assert!(!j.publish(a, pix()) && j.done.is_none()); // a's picture is dropped
        assert!(j.publish(j.gen, None) && matches!(j.done, Some(None))); // b: no preview
        let b = j.gen;
        j.want(None); // closed
        assert!(j.gen != b && j.req.is_none() && j.done.is_none());
        assert!(!j.publish(b, pix()) && j.done.is_none());
    }

    #[test]
    fn peek_alpha() {
        let mut photo = [10u8, 20, 30, 0, 40, 50, 60, 0];
        fix_alpha(&mut photo);
        assert_eq!(photo, [10u8, 20, 30, 255, 40, 50, 60, 255]);
        let mut icon = [0u8, 0, 0, 0, 40, 50, 60, 128];
        fix_alpha(&mut icon);
        assert_eq!(icon, [0u8, 0, 0, 0, 40, 50, 60, 128]); // real alpha: kept, transparent corners too
    }

    #[test]
    fn folder_size_text() {
        let kb = human(1024);
        assert_eq!(size_text(false, 0, true, None, false), " (calculating…)");
        assert_eq!(size_text(false, 0, true, Some((0, false, false)), false), " (calculating…)");
        assert_eq!(size_text(false, 1024, true, Some((1024, false, false)), false), format!(" ({}…)", human(2048)));
        assert_eq!(size_text(false, 0, true, Some((1024, true, false)), false), format!(" ({kb})"));
        assert_eq!(size_text(false, 0, true, Some((1024, true, true)), false), format!(" ({kb}, partial)"));
        // something missed while collecting the selection isn't shown as a full total
        assert_eq!(size_text(false, 0, true, Some((1024, true, false)), true), format!(" ({kb}, partial)"));
        assert_eq!(size_text(true, 0, true, Some((1024, true, false)), true), format!(" ({kb}, 일부만)"));
        assert_eq!(size_text(false, 1024, false, None, false), format!(" ({kb})"));
        assert_eq!(size_text(false, 1024, false, None, true), format!(" ({kb}, partial)"));
        assert_eq!(size_text(false, 0, false, None, false), "");
        assert_eq!(size_text(false, 0, false, None, true), " (size unknown)");
        assert_eq!(size_text(true, 0, false, None, true), " (크기 모름)");
    }

    #[test]
    fn folder_size_collect() {
        let root = tree("collect", &[("f.bin".into(), 7), (r"d\x.bin".into(), 1)]);
        let path = |p: &str| root.join(p).to_string_lossy().into_owned();
        let (f, d) = (path("f.bin"), path("d"));
        let none = Sel { file_bytes: 0, dirs: vec![], partial: true };
        assert_eq!(collect_sel(None, |_| None), none); // GetSelection / GetCount failed
        assert_eq!(collect_sel(Some(1), |_| None), none); // GetItemAt / no file system path
        assert_eq!(collect_sel(Some(1), |_| Some(path("gone"))), none);
        assert_eq!(collect_sel(Some(1), |_| Some(r"C:\".into())), none); // a drive root isn't walked
        let both = [f.clone(), d.clone()];
        assert_eq!(collect_sel(Some(2), |k| Some(both[k as usize].clone())), Sel { file_bytes: 7, dirs: vec![d], partial: false });
        let calls = std::cell::Cell::new(0);
        let s = collect_sel(Some(10_001), |_| {
            calls.set(calls.get() + 1);
            Some(f.clone())
        });
        assert_eq!((s.file_bytes, s.partial, calls.get()), (70_000, true, 10_000));
        _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn shortcut_rows() {
        let (en, ko) = (key_rows(KEYS_EN), key_rows(KEYS_KO));
        assert_eq!([en[0].len(), en[1].len()], [ko[0].len(), ko[1].len()]);
        assert!(en[0].len() >= 10 && en[1].len() >= 5);
        for r in en.iter().chain(&ko).flatten() {
            assert!(!r.desc.is_empty() && !r.keys.is_empty() && r.keys.iter().all(|k| !k.is_empty()));
        }
        let caps = |k: &str| en[0].iter().find(|r| r.keys.join(", ").starts_with(k)).map(|r| r.keys.len());
        assert_eq!([caps("Ctrl+L"), caps("F2"), caps("Ctrl+1"), caps("Ctrl+/")], [Some(3), Some(4), Some(1), Some(1)]);
        assert_eq!(en[0].iter().find(|r| r.keys[0] == "F2").unwrap().keys, ["F2", "Del", "Ctrl+C/X/V", "F5"]);
        // the last mouse line has no "name: " and continues the one above
        assert!(en[1].last().unwrap().desc.ends_with("If Grids is already running, they open in that window."));
        assert!(ko[1].last().unwrap().desc.ends_with("이미 실행 중이면 그 창에서 엽니다."));
    }

    #[test]
    fn held_keys() {
        let (enter, a) = (VK_RETURN.0, VK_A.0);
        assert_eq!(held_key(Some(enter), enter, true), (true, Some(enter))); // its repeat: swallowed
        assert_eq!(held_key(Some(enter), enter, false), (false, None)); // a fresh press: keyup was missed
        assert_eq!(held_key(Some(enter), a, false), (false, Some(enter)));
        assert_eq!(held_key(Some(enter), a, true), (false, Some(enter)));
        assert_eq!(held_key(None, enter, true), (false, None));
    }

    #[test]
    fn match_box_fits() {
        for dpi in [96, 144, 192] {
            let s = |v: i32| v * dpi / 96;
            for label_w in [0, s(40), s(400)] {
                let g = match_geo(label_w, s(16), dpi);
                assert!(g.w >= s(320) && g.label.right - g.label.left == label_w);
                assert!(g.field.left >= g.label.right && g.field.right - g.field.left >= s(120) && g.field.right < g.w);
                assert!(g.edit.left > g.field.left && g.edit.right < g.field.right);
                assert!(g.edit.top >= g.field.top && g.edit.bottom <= g.field.bottom && g.field.bottom < g.h);
            }
        }
    }

    #[test]
    fn shortcut_layout() {
        for dpi in [96, 192] {
            for dip in [300, 760] {
                let (cw, lh) = (7 * dpi / 96, 16 * dpi / 96);
                let measure = |t: &str, max_w: i32, cap: bool| {
                    let w = t.chars().count() as i32 * cw;
                    if cap {
                        (w, lh)
                    } else {
                        let lines = t.split('\n').map(|l| ((l.chars().count() as i32 * cw + max_w - 1) / max_w).max(1)).sum::<i32>();
                        (w.min(max_w), lines * lh)
                    }
                };
                for text in [KEYS_EN, KEYS_KO] {
                    let g = keys_layout(&key_rows(text), dip * dpi / 96, dpi, &measure);
                    let (pad, gap) = (KEYS_PAD * dpi / 96, 16 * dpi / 96);
                    let mut prev = 0;
                    for r in &g.rows {
                        assert!(r.row.top >= prev, "rows overlap at {dpi} dpi, {dip} dip");
                        for k in &r.keys {
                            // key column: from the padding to a gap before the description
                            assert!(k.left >= pad && k.right + gap <= r.desc.left && k.bottom <= r.row.bottom);
                        }
                        let cap_lines = r.keys.iter().map(|k| k.bottom).max().unwrap() - r.row.top;
                        assert!(r.row.bottom - r.row.top >= cap_lines.max(r.desc.bottom - r.desc.top));
                        assert!(r.desc.bottom <= r.row.bottom && r.desc.right <= dip * dpi / 96 - pad);
                        prev = r.row.bottom;
                    }
                    assert!(prev <= g.h && g.titles[1].top > g.titles[0].bottom);
                }
            }
        }
    }

    #[test]
    fn window_fits_work_area() {
        let r = |left, top, right, bottom| RECT { left, top, right, bottom };
        let work = r(0, 0, 1024, 728);
        let inside = r(100, 50, 900, 650);
        assert_eq!(fit(inside, work), inside);
        // the 1400×900 default on a 1024×768 screen
        assert_eq!(fit(r(26, 26, 1426, 926), work), work);
        // spilling right and down: same size, slid back in
        assert_eq!(fit(r(500, 400, 1300, 1000), work), r(224, 128, 1024, 728));
        // a secondary monitor left of the primary
        let left_mon = r(-1920, 0, 0, 1040);
        assert_eq!(fit(r(-2100, -50, -1300, 550), left_mon), r(-1920, 0, -1120, 600));
        assert_eq!(fit(r(-300, 100, 500, 700), left_mon), r(-800, 100, 0, 700));
        // taskbar on top or on the left: a saved rect that fits comes back unchanged, so it never creeps
        let mon = r(0, 0, 1920, 1080);
        for work in [r(0, 40, 1920, 1080), r(60, 0, 1920, 1080)] {
            for saved in [r(0, 0, 800, 600), r(100, 100, 900, 700), r(1060, 400, 1860, 1000), r(1120, 440, 1920, 1040)] {
                if saved.right + work.left <= work.right && saved.bottom + work.top <= work.bottom {
                    assert_eq!(fit_saved(saved, mon, work), saved);
                }
            }
            // too big or off screen: in screen coordinates the result lies inside rcWork
            for saved in [r(-300, -200, 3000, 2000), r(1500, 900, 2300, 1500)] {
                let f = fit_saved(saved, mon, work);
                let (dx, dy) = (work.left - mon.left, work.top - mon.top);
                assert_eq!(fit(r(f.left + dx, f.top + dy, f.right + dx, f.bottom + dy), work), r(f.left + dx, f.top + dy, f.right + dx, f.bottom + dy));
            }
        }
        assert_eq!(fit_saved(r(1500, 900, 2300, 1500), mon, r(0, 40, 1920, 1080)), r(1120, 440, 1920, 1040));
    }
}
