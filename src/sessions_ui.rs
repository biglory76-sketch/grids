use crate::*;

// ---------- sessions ----------

pub(crate) unsafe fn save_session() {
    let (main, n) = app(|a| (a.main, a.s.panes()));
    for i in 0..n {
        save_view(i);
    }
    let mut wp = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
    _ = GetWindowPlacement(main, &mut wp);
    let ini = app(|a| {
        let r = wp.rcNormalPosition;
        let max = (wp.showCmd == SW_SHOWMAXIMIZED.0 as u32) as i32;
        a.s.win = Some([r.left, r.top, r.right, r.bottom, max]);
        a.s.to_ini()
    });
    _ = std::fs::write(ini_path(), ini);
}

// Save button / Ctrl+S: overwrite the current session file; the first time, ask for a name.
pub(crate) unsafe fn save_current() {
    let file = app(|a| a.s.file.clone());
    if file.is_empty() {
        _ = save_session_as();
    } else {
        write_session(Path::new(&file));
    }
}

pub(crate) unsafe fn write_session(path: &Path) {
    for i in 0..app(|a| a.s.panes()) {
        save_view(i);
    }
    if std::fs::write(path, app(|a| a.s.snapshot())).is_err() {
        _ = MessageBeep(MB_ICONWARNING);
        return;
    }
    app(|a| {
        a.s.file = path.to_string_lossy().into_owned();
        (a.base, a.dirty) = (Some(a.s.clone()), false);
    });
    refresh_save_button();
}

// "New name": the save dialog does naming, overwrite prompts, rename and delete for us.
pub(crate) unsafe fn save_session_as() -> Result<()> {
    let main = app(|a| a.main);
    let dir = sessions_dir();
    _ = std::fs::create_dir_all(&dir);
    let dlg: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?;
    let kind = HSTRING::from(tr("Grids session (*.grids)", "Grids 세션 (*.grids)"));
    dlg.SetFileTypes(&[COMDLG_FILTERSPEC { pszName: PCWSTR(kind.as_ptr()), pszSpec: w!("*.grids") }])?;
    dlg.SetDefaultExtension(w!("grids"))?;
    dlg.SetTitle(&HSTRING::from(tr("Save session", "세션 저장")))?;
    if let Ok(f) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(dir.to_string_lossy().as_ref()), None) {
        dlg.SetFolder(&f)?;
    }
    dlg.Show(Some(main))?; // Err on cancel
    let path = pwstr(dlg.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?);
    write_session(Path::new(&path));
    Ok(())
}

pub(crate) unsafe fn load_session_file(path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else {
        _ = MessageBeep(MB_ICONWARNING);
        return;
    };
    for i in 0..MAXP {
        destroy_pane(i);
    }
    let other = Session::parse(&text);
    app(|a| {
        a.s.take_layout(&other);
        a.zoom = None;
        a.s.file = path.to_string_lossy().into_owned();
        (a.base, a.dirty) = (Some(a.s.clone()), false);
    });
    refresh_save_button();
    relayout();
    redraw_all();
    focus_view(app(|a| a.s.active));
}

// Paths from the command line or a second instance: a session file loads, folders fill panes from the active one on.
pub(crate) unsafe fn open_args(args: Vec<String>) {
    if let Some(f) = session_arg(&args) {
        return load_session_file(Path::new(f));
    }
    if app(|x| x.zoom.take()).is_some() {
        relayout();
        redraw_all();
    }
    let (a, n) = app(|x| (x.s.active, x.s.panes()));
    for (k, p) in args.iter().take(n).enumerate() {
        go_near((a + k) % n, p);
    }
    focus_view(a);
}

// Window of a running copy of this same exe. Portable copies elsewhere have their own ini and stay separate.
pub(crate) unsafe fn running_instance() -> Option<HWND> {
    let me = std::env::current_exe().ok()?.to_string_lossy().to_lowercase();
    let mut h = None;
    loop {
        let w = FindWindowExW(None, h, w!("GridsMain"), PCWSTR::null()).ok()?;
        h = Some(w);
        let mut pid = 0;
        GetWindowThreadProcessId(w, Some(&mut pid));
        let Ok(p) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { continue };
        let (mut buf, mut n) = ([0u16; 1024], 1024u32);
        let ok = QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut n).is_ok();
        _ = CloseHandle(p);
        if ok && String::from_utf16_lossy(&buf[..n as usize]).to_lowercase() == me {
            return Some(w);
        }
    }
}

#[repr(C)]
pub(crate) struct CopyData {
    pub(crate) dw: usize,
    pub(crate) cb: u32,
    pub(crate) lp: *const core::ffi::c_void,
}


// Repaints the save icon and puts the session name / unsaved state into its tooltip.
pub(crate) unsafe fn refresh_save_button() {
    let (main, tip, file, dirty) = app(|a| (a.main, a.tip, a.s.file.clone(), a.dirty));
    let Ok(b) = GetDlgItem(Some(main), ID_TB_SAVE) else { return };
    _ = InvalidateRect(Some(b), None, false);
    let mut hint = match Path::new(&file).file_stem() {
        Some(n) if KO.load(Ordering::Relaxed) => format!("'{}'에 저장 (Ctrl+S)", n.to_string_lossy()),
        Some(n) => format!("Save to '{}' (Ctrl+S)", n.to_string_lossy()),
        None => tr("Save session (Ctrl+S)", "세션 저장 (Ctrl+S)").into(),
    };
    if dirty {
        hint += tr(" · unsaved layout changes", " · 저장 안 된 배치 변경");
    }
    let hint = HSTRING::from(hint);
    let ti = TTTOOLINFOW {
        cbSize: size_of::<TTTOOLINFOW>() as u32,
        uFlags: TTF_IDISHWND,
        hwnd: main,
        uId: b.0 as usize,
        lpszText: PWSTR(hint.as_ptr() as *mut u16),
        ..Default::default()
    };
    SendMessageW(tip, TTM_UPDATETIPTEXTW, None, Some(LPARAM(&ti as *const _ as isize)));
}

// Polled with the status timer: view modes are only observable by asking the views.
pub(crate) unsafe fn check_dirty() {
    for i in 0..app(|a| a.s.panes()) {
        save_view(i);
    }
    let changed = app(|a| {
        if let Some(b) = a.base.as_mut() {
            for i in 0..a.s.panes() {
                if b.cols[i].is_empty() {
                    b.cols[i] = a.s.cols[i].clone();
                }
            }
        }
        let d = a.base.as_ref().is_some_and(|b| layout_differs(&a.s, b));
        std::mem::replace(&mut a.dirty, d) != d
    });
    if changed {
        refresh_save_button();
    }
}
