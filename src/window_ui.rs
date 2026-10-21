use crate::*;

// ---------- window procs ----------

pub(crate) fn xy(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xffff) as i16 as i32, ((lp.0 >> 16) & 0xffff) as i16 as i32)
}

pub(crate) unsafe fn cursor_for(h: HWND) -> Option<Hit> {
    let mut pt = POINT::default();
    _ = GetCursorPos(&mut pt);
    _ = ScreenToClient(h, &mut pt);
    app(|a| hit(&geo(a), pt.x, pt.y))
}

pub(crate) extern "system" fn main_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            // While the user drags the window edge, lay the panes out once at the end (same cost reason as splitters).
            WM_SIZE => {
                let sizing = app(|a| {
                    if wp.0 as u32 != SIZE_MINIMIZED {
                        a.was_max = wp.0 as u32 == SIZE_MAXIMIZED;
                    }
                    a.sizing
                });
                if !sizing {
                    relayout();
                    redraw_all(); // maximize / restore
                }
            }
            WM_ENTERSIZEMOVE => app(|a| a.sizing = true),
            WM_EXITSIZEMOVE => {
                app(|a| a.sizing = false);
                relayout();
                redraw_all();
            }
            WM_PAINT => paint_main(h),
            WM_ERASEBKGND => return LRESULT(1), // paint_main covers everything
            WM_DRAWITEM => {
                draw_button(&*(lp.0 as *const DRAWITEMSTRUCT));
                return LRESULT(1);
            }
            WM_TIMER if wp.0 == TIMER_REVEAL => {
                _ = KillTimer(Some(h), TIMER_REVEAL);
                reveal_tree_selection();
                let left = app(|a| {
                    a.reveal_tries = a.reveal_tries.saturating_sub(1);
                    a.reveal_tries
                });
                if left > 0 {
                    SetTimer(Some(h), TIMER_REVEAL, if left == 2 { 330 } else { 550 }, None);
                }
            }
            WM_TIMER if wp.0 == TIMER_STATUS => {
                update_status();
                check_dirty();
                autosave();
            }
            WM_SETCURSOR if wp.0 == h.0 as usize && (lp.0 & 0xffff) as u32 == HTCLIENT => {
                let Some(d) = app(|a| a.drag).or_else(|| cursor_for(h)) else {
                    return DefWindowProcW(h, msg, wp, lp);
                };
                let c = match d {
                    Hit::Side | Hit::Col(_) => IDC_SIZEWE,
                    Hit::Row(..) => IDC_SIZENS,
                };
                SetCursor(LoadCursorW(None, c).ok());
                return LRESULT(1);
            }
            WM_LBUTTONDOWN => {
                let (x, y) = xy(lp);
                if let Some(d) = app(|a| hit(&geo(a), x, y)) {
                    app(|a| a.drag = Some(d));
                    SetCapture(h);
                }
            }
            WM_MOUSEMOVE => {
                let (x, y) = xy(lp);
                let moved = app(|a| {
                    let d = a.drag?;
                    let g = geo(a);
                    let min = 60 * a.dpi / 96;
                    match d {
                        Hit::Side => a.s.side = x - g.pad - g.gap / 2,
                        Hit::Col(k) => drag_ratio(&mut a.s.colr, k, x - g.gap / 2, g.gx, g.w - g.pad - g.gx, g.gap, min),
                        Hit::Row(c, k) => drag_ratio(&mut a.s.rowr[c], k, y - g.gap / 2, g.top, g.bot - g.top, g.gap, min),
                    }
                    a.s.side = kept_side(a.s.tree, a.s.side, geo(a).side);
                    Some(())
                });
                if moved.is_some() {
                    show_bars(); // panes stay put until release: relaying out 16 Explorer views per mouse move is ~120ms
                }
            }
            WM_LBUTTONUP => {
                _ = ReleaseCapture();
                end_drag();
            }
            WM_CAPTURECHANGED => end_drag(),
            WM_LBUTTONDBLCLK => {
                let (x, y) = xy(lp);
                let d = app(|a| hit(&geo(a), x, y));
                app(|a| match d {
                    Some(Hit::Side) => a.s.side = 240 * a.dpi / 96,
                    Some(Hit::Col(_)) => a.s.colr = even(a.s.heights.len()),
                    Some(Hit::Row(c, _)) => a.s.rowr[c] = even(a.s.heights[c]),
                    None => {}
                });
                relayout();
                redraw_all();
            }
            WM_COMMAND => match wp.0 & 0xffff {
                id if id == ID_TB_LAYOUT as usize => open_grid(),
                id if id == ID_TB_SAVE as usize => save_current(),
                id if id == ID_TB_LOAD as usize => open_load(),
                id if id == ID_TB_FAV as usize => open_favs(),
                id if id == ID_TB_THEME as usize => {
                    app(|a| a.s.dark ^= true);
                    apply_theme();
                }
                id if id == ID_TB_VIEW as usize => popup_menu(app(|a| a.view_menu), ID_TB_VIEW),
                ID_TREE => toggle_tree(),
                ID_HIDDEN => toggle_hidden(),
                ID_EXT => toggle_extensions(),
                ID_DBLUP => app(|a| a.s.dblup ^= true),
                ID_LANG => set_language(!KO.load(Ordering::Relaxed)),
                ID_HELP => open_help(),
                ID_ABOUT => about(h),
                id @ ID_ACCENT..=1405 => set_accent(pal().accents[id - ID_ACCENT].1),
                ID_ACCENT_PICK => pick_accent(),
                ID_KEYS => open_keys(),
                _ => {}
            },
            WM_INITMENUPOPUP => {
                let (menu, dblup, tree, accent) = app(|a| (a.view_menu, a.s.dblup, a.s.tree, a.s.accent_now()));
                let check = |id: usize, on: bool| CheckMenuItem(menu, id as u32, (MF_BYCOMMAND | if on { MF_CHECKED } else { MF_UNCHECKED }).0);
                check(ID_TREE, tree);
                check(ID_HIDDEN, hidden());
                check(ID_EXT, extensions());
                check(ID_DBLUP, dblup);
                check(ID_LANG, KO.load(Ordering::Relaxed));
                for (k, (_, c)) in pal().accents.iter().enumerate() {
                    check(ID_ACCENT + k, *c == accent);
                }
            }
            WM_COPYDATA => {
                let cd = &*(lp.0 as *const CopyData);
                if cd.dw != COPY_ID || cd.lp.is_null() {
                    return LRESULT(0);
                }
                // copy first: the sender's buffer is only valid during this call
                let text = String::from_utf16_lossy(std::slice::from_raw_parts(cd.lp as *const u16, cd.cb as usize / 2));
                open_args(text.split('\n').map(String::from).collect());
                return LRESULT(1);
            }
            WM_DPICHANGED => {
                let dpi = (wp.0 & 0xffff) as i32;
                app(|a| {
                    a.s.side = a.s.side * dpi / a.dpi;
                    a.dpi = dpi;
                });
                apply_font();
                let r = &*(lp.0 as *const RECT);
                _ = SetWindowPos(h, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            }
            WM_CLOSE => {
                save_session();
                _ = DestroyWindow(h);
            }
            // Windows restart / sign-out: no WM_CLOSE comes. WM_QUERYENDSESSION stays with DefWindowProcW (never blocks).
            WM_ENDSESSION if wp.0 != 0 => save_session(),
            WM_DESTROY => {
                _ = KillTimer(Some(h), TIMER_STATUS);
                for i in 0..MAXP {
                    destroy_pane(i);
                }
                app(|a| a.tree = None);
                PostQuitMessage(0);
            }
            _ => return DefWindowProcW(h, msg, wp, lp),
        }
        LRESULT(0)
    }
}

pub(crate) extern "system" fn pane_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let nav = |flags: u32| {
            if let Some(i) = pane_of(h) {
                set_active(i);
                browse(i, flags);
                focus_view(i);
            }
        };
        match msg {
            WM_TIMER if wp.0 == TIMER_SETTLE => settle_cols(h),
            WM_SIZE => layout_pane(h),
            WM_LBUTTONDBLCLK => {
                let (x, y) = ((lp.0 & 0xffff) as i16 as i32, (lp.0 >> 16 & 0xffff) as i16 as i32);
                let addr = addr_rect(h, app(|a| a.dpi));
                let on_addr = x >= addr.left && x < addr.right && y >= addr.top && y < addr.bottom;
                if let Some(i) = pane_of(h).filter(|_| !on_addr) {
                    set_active(i);
                    toggle_zoom();
                }
            }
            // address bar crumbs (set_active already ran in pre_translate)
            WM_LBUTTONDOWN => {
                if let Some(i) = pane_of(h) {
                    crumb_click(h, i, (lp.0 & 0xffff) as i16 as i32, (lp.0 >> 16 & 0xffff) as i16 as i32);
                }
            }
            WM_MOUSEMOVE => {
                let (x, y) = ((lp.0 & 0xffff) as i16 as i32, (lp.0 >> 16 & 0xffff) as i16 as i32);
                let k = pane_of(h).and_then(|i| crumb_at(h, i, x, y)).flatten().map(|(k, ..)| k);
                crumb_hover(h, k);
            }
            WM_MOUSELEAVE => {
                if app(|a| a.crumb_hot.is_some_and(|(w, _)| w == h)) {
                    crumb_hover(h, None);
                }
            }
            WM_PAINT => paint_pane(h),
            // folder dropped on the top bar: still this pane's? then open it here, as ⇄ does
            WM_DROP_OPEN => {
                let queued = app(|a| {
                    let mine = a.drop_open.as_ref().is_some_and(|(w, i, _)| *w == h && a.panes[*i].as_ref().is_some_and(|p| p.hwnd == h));
                    if mine {
                        a.drop_open.take()
                    } else {
                        None
                    }
                });
                if let Some((_, i, item)) = queued {
                    browse_item(i, &item);
                    set_active(i);
                    focus_view(i);
                }
            }
            WM_ERASEBKGND => return LRESULT(1), // paint_pane covers everything
            WM_DRAWITEM => {
                draw_button(&*(lp.0 as *const DRAWITEMSTRUCT));
                return LRESULT(1);
            }
            WM_CTLCOLOREDIT => return edit_colors(wp),
            WM_COMMAND => match ((wp.0 & 0xffff) as i32, ((wp.0 >> 16) & 0xffff) as u32) {
                // pill gets an accent outline while its edit has focus
                (ID_EDIT, EN_SETFOCUS | EN_KILLFOCUS) => sync_addr(h),
                (ID_FILTER, EN_SETFOCUS | EN_KILLFOCUS) => _ = InvalidateRect(Some(h), None, false),
                (ID_BACK, _) => nav(SBSP_NAVIGATEBACK),
                (ID_FWD, _) => nav(SBSP_NAVIGATEFORWARD),
                (ID_UP, _) => nav(SBSP_PARENT),
                (ID_OPEN_IN, _) => {
                    if let Some(i) = pane_of(h) {
                        open_in_menu(h, i);
                    }
                }
                (ID_FAV, _) => {
                    if let Some(i) = pane_of(h) {
                        toggle_fav(i);
                    }
                }
                // typing in the filter box applies after a short pause
                (ID_FILTER, EN_CHANGE) => _ = SetTimer(Some(h), TIMER_FILTER, 250, None),
                _ => {}
            },
            WM_TIMER if wp.0 == TIMER_FILTER => {
                _ = KillTimer(Some(h), TIMER_FILTER);
                if let Some(i) = pane_of(h) {
                    apply_filter(i);
                }
            }
            // right-click on ← / →: recent folders; anywhere else as before
            WM_CONTEXTMENU if [ID_BACK, ID_FWD].iter().any(|&id| GetDlgItem(Some(h), id).is_ok_and(|b| b.0 as usize == wp.0)) => {
                if let Some(i) = pane_of(h) {
                    set_active(i);
                    recent_menu(i);
                }
            }
            // right-click on the header itself (not its buttons or fields): colour label
            WM_CONTEXTMENU if wp.0 == h.0 as usize => {
                let mut p = POINT { x: (lp.0 & 0xffff) as i16 as i32, y: (lp.0 >> 16 & 0xffff) as i16 as i32 };
                let screen = p;
                _ = ScreenToClient(h, &mut p);
                let mut rc = RECT::default();
                _ = GetClientRect(h, &mut rc);
                match pane_of(h) {
                    Some(i) if lp.0 != -1 && p.y < pane_geo(rc.right, rc.bottom, app(|a| a.dpi)).view.top => tint_menu(h, i, screen),
                    _ => return DefWindowProcW(h, msg, wp, lp),
                }
            }
            // mouse side buttons
            WM_APPCOMMAND => match (lp.0 >> 16) & 0x0fff {
                1 => nav(SBSP_NAVIGATEBACK),
                2 => nav(SBSP_NAVIGATEFORWARD),
                _ => return DefWindowProcW(h, msg, wp, lp),
            },
            _ => return DefWindowProcW(h, msg, wp, lp),
        }
        LRESULT(0)
    }
}

// The README on GitHub, in the UI language.
pub(crate) unsafe fn open_help() {
    let url = format!("{HOME}/blob/main/{}", tr("README.md", "README.ko.md"));
    ShellExecuteW(None, w!("open"), &HSTRING::from(url), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
}

pub(crate) unsafe fn about(owner: HWND) {
    let text = format!(
        "Grids {}\n{}\n\n{}\n{HOME}\n\n{}",
        env!("CARGO_PKG_VERSION"),
        tr("Fast multi-pane file manager for Windows", "Windows용 빠른 다중 패널 파일 관리자"),
        tr("MIT License · © 2026 efww", "MIT 라이선스 · © 2026 efww"),
        tr("Open the project page?", "프로젝트 페이지를 열까요?"),
    );
    if MessageBoxW(Some(owner), &HSTRING::from(text), &HSTRING::from(tr("About Grids", "Grids 정보")), MB_YESNO | MB_ICONINFORMATION) == IDYES {
        ShellExecuteW(None, w!("open"), &HSTRING::from(HOME), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

// Toolbar buttons: id, accessible name, tooltip.
pub(crate) fn toolbar_tips() -> [(i32, &'static str, &'static str); 6] {
    [
        (ID_TB_LAYOUT, "", tr("Layout: click = even grid, drag = rows per column", "레이아웃: 클릭 = 균등, 끌기 = 열마다 칸 수")),
        (ID_TB_SAVE, tr("Save session", "세션 저장"), ""), // tooltip set by refresh_save_button
        (ID_TB_LOAD, tr("Load", "불러오기"), tr("Load a session", "세션 불러오기")),
        (ID_TB_VIEW, tr("View", "보기"), tr("View: hidden files · extensions · colours · language · help", "보기: 숨김 파일 · 확장명 · 선택 색 · 언어 · 도움말")),
        (ID_TB_FAV, tr("Favourites", "즐겨찾기"), tr("Favourite folders (Ctrl+B; open in the active pane)", "즐겨찾기 폴더 (Ctrl+B, 선택한 패널에서 열기)")),
        (ID_TB_THEME, tr("Theme", "테마"), tr("Light / dark theme", "라이트 / 다크 테마")),
    ]
}

// Switches the UI language now: menu, toolbar tooltips, and the panes (rebuilt with their tooltips).
pub(crate) unsafe fn set_language(ko: bool) {
    KO.store(ko, Ordering::Relaxed);
    let (main, old) = app(|a| {
        a.s.ko = ko;
        (a.main, a.view_menu)
    });
    if let Ok(m) = make_menus() {
        app(|a| a.view_menu = m);
        _ = DestroyMenu(old);
    }
    for (id, _, hint) in toolbar_tips() {
        if let Ok(b) = GetDlgItem(Some(main), id) {
            tip_text(b, hint, TTM_UPDATETIPTEXTW);
        }
    }
    refresh_save_button();
    app(|a| a.status_key.clear());
    apply_theme();
}

// The view popup opened from the top toolbar (no classic menu bar).
pub(crate) unsafe fn make_menus() -> Result<HMENU> {
    let view = CreatePopupMenu()?;
    let item = |menu: HMENU, flags: MENU_ITEM_FLAGS, id: usize, text: &str| AppendMenuW(menu, flags, id, &HSTRING::from(text));
    item(view, MF_STRING, ID_TREE, tr("Folder tree\tCtrl+Shift+E", "폴더 트리\tCtrl+Shift+E"))?;
    item(view, MF_STRING, ID_HIDDEN, tr("Show hidden files\tCtrl+H", "숨김 파일 표시\tCtrl+H"))?;
    item(view, MF_STRING, ID_EXT, tr("Show file name extensions", "파일 확장명 표시"))?;
    item(view, MF_STRING, ID_DBLUP, tr("Double-click empty space: parent folder", "빈 곳 더블클릭: 위 폴더로"))?;
    let colors = CreatePopupMenu()?;
    for (k, (name, _)) in LIGHT.accents.iter().enumerate() {
        item(colors, MF_STRING, ID_ACCENT + k, tr(ACCENT_EN[k], name))?;
    }
    AppendMenuW(colors, MF_SEPARATOR, 0, PCWSTR::null())?;
    item(colors, MF_STRING, ID_ACCENT_PICK, tr("Custom...", "직접 선택..."))?;
    item(view, MF_POPUP, colors.0 as usize, tr("Selection colour", "선택 색"))?;
    AppendMenuW(view, MF_SEPARATOR, 0, PCWSTR::null())?;
    item(view, MF_STRING, ID_LANG, "한국어 (Korean)")?;
    AppendMenuW(view, MF_SEPARATOR, 0, PCWSTR::null())?;
    item(view, MF_STRING, ID_KEYS, tr("Keyboard shortcuts\tCtrl+/", "단축키\tCtrl+/"))?;
    item(view, MF_STRING, ID_HELP, tr("Help (online)\tF1", "도움말 (온라인)\tF1"))?;
    item(view, MF_STRING, ID_ABOUT, tr("About Grids", "Grids 정보"))?;
    Ok(view)
}


pub(crate) unsafe fn popup_menu(menu: HMENU, id: i32) {
    let main = app(|a| a.main);
    let mut rc = RECT::default();
    if let Ok(b) = GetDlgItem(Some(main), id) {
        _ = GetWindowRect(b, &mut rc);
    }
    _ = TrackPopupMenu(menu, TPM_LEFTALIGN | TPM_TOPALIGN, rc.left, rc.bottom + 4, None, main, None);
}
