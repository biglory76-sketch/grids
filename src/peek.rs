use crate::*;

// ---------- F3 quick preview ----------

#[derive(Clone)]
pub(crate) struct PeekReq {
    pub(crate) path: String,
    pub(crate) w: i32,
    pub(crate) h: i32,
    pub(crate) hwnd: isize, // popup told by WM_PEEK_DONE
}

pub(crate) struct Pix {
    pub(crate) w: i32,
    pub(crate) h: i32,
    pub(crate) bgra: Vec<u8>, // premultiplied, top-down
    pub(crate) size: Option<u64>,
}

// One worker, newest request only. gen moves on every want, so a result for an item already left
// (or a closed popup) is dropped instead of shown.
pub(crate) struct PeekJob {
    pub(crate) gen: usize,
    pub(crate) req: Option<PeekReq>,
    pub(crate) done: Option<Option<Pix>>, // Some(None) = no preview
}

impl PeekJob {
    pub(crate) fn want(&mut self, req: Option<PeekReq>) {
        (self.gen, self.req, self.done) = (self.gen + 1, req, None);
    }

    pub(crate) fn publish(&mut self, gen: usize, r: Option<Pix>) -> bool {
        if self.gen != gen {
            return false;
        }
        self.done = Some(r);
        true
    }
}

pub(crate) static PEEK: Mutex<PeekJob> = Mutex::new(PeekJob { gen: 0, req: None, done: None });
pub(crate) static PEEK_WAKE: Condvar = Condvar::new();

pub(crate) fn peek_job() -> std::sync::MutexGuard<'static, PeekJob> {
    PEEK.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn want_peek(req: Option<PeekReq>) {
    let wake = req.is_some();
    peek_job().want(req);
    if wake {
        static WORKER: Once = Once::new();
        WORKER.call_once(|| _ = std::thread::Builder::new().spawn(peek_worker));
        PEEK_WAKE.notify_one();
    }
}

// Lives until the process ends. COM is up only while one picture is made; the lock is never held then.
pub(crate) fn peek_worker() {
    let mut seen = 0;
    loop {
        let (gen, req) = {
            let mut j = peek_job();
            while j.req.is_none() || j.gen == seen {
                j = PEEK_WAKE.wait(j).unwrap_or_else(|e| e.into_inner());
            }
            (j.gen, j.req.clone().unwrap())
        };
        seen = gen;
        let pix = unsafe {
            let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
            let pix = peek_pixels(&req);
            if com {
                CoUninitialize();
            }
            pix
        };
        if peek_job().publish(gen, pix) {
            unsafe { _ = PostMessageW(Some(HWND(req.hwnd as _)), WM_PEEK_DONE, WPARAM(gen), LPARAM(0)) };
        }
    }
}

// The shell's thumbnail (or its icon when there is none), copied out so no GDI or COM object leaves this call.
pub(crate) unsafe fn peek_pixels(req: &PeekReq) -> Option<Pix> {
    let f: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(&req.path), None).ok()?;
    let bmp = f.GetImage(SIZE { cx: req.w, cy: req.h }, SIIGBF_RESIZETOFIT).ok()?;
    let mut bm = BITMAP::default();
    GetObjectW(bmp.into(), size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as _));
    let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
    let mut bgra = vec![0u8; (w.max(0) * h * 4) as usize];
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let dc = CreateCompatibleDC(None);
    let rows = if bgra.is_empty() { 0 } else { GetDIBits(dc, bmp, 0, h as u32, Some(bgra.as_mut_ptr() as _), &mut bmi, DIB_RGB_COLORS) };
    _ = DeleteDC(dc);
    _ = DeleteObject(bmp.into());
    if rows != h || rows == 0 {
        return None;
    }
    fix_alpha(&mut bgra);
    let size = std::fs::metadata(&req.path).ok().filter(|m| m.is_file()).map(|m| m.len());
    Some(Pix { w, h, bgra, size })
}

// Photos come back with every alpha byte 0: make them opaque. Anything with real alpha is left alone.
pub(crate) fn fix_alpha(bgra: &mut [u8]) {
    if bgra.chunks(4).all(|p| p.get(3) == Some(&0)) {
        bgra.chunks_mut(4).for_each(|p| p[3] = 255);
    }
}

// [picture, name, size] inside the popup's client rect.
pub(crate) fn peek_geo(rc: RECT, dpi: i32) -> [RECT; 3] {
    let s = |v: i32| v * dpi / 96;
    let (l, r) = (rc.left + s(16), rc.right - s(16));
    let size = RECT { left: l, right: r, top: rc.bottom - s(12) - s(18), bottom: rc.bottom - s(12) };
    let name = RECT { left: l, right: r, top: size.top - s(22), bottom: size.top };
    [RECT { left: l, right: r, top: rc.top + s(16), bottom: name.top - s(8) }, name, size]
}

pub(crate) unsafe fn open_peek(i: usize) {
    let Some(fv) = view_of::<IFolderView2>(i) else { return };
    let (main, dpi, open) = app(|a| (a.main, a.dpi, a.peek));
    if !open.0.is_null() {
        return;
    }
    if fv.GetFocusedItem().unwrap_or(-1) < 0 {
        _ = MessageBeep(MB_ICONWARNING);
        return;
    }
    let s = |v: i32| v * dpi / 96;
    let mut rc = RECT::default();
    _ = GetClientRect(main, &mut rc);
    let (w, h) = ((rc.right * 7 / 10).min(s(900)), (rc.bottom * 7 / 10).min(s(700)));
    let mut pt = POINT { x: (rc.right - w) / 2, y: (rc.bottom - h) / 2 };
    _ = ClientToScreen(main, &mut pt);
    let Ok(h) = CreateWindowExW(WS_EX_TOOLWINDOW, w!("GridsPeek"), w!(""), WS_POPUP | WS_VISIBLE, pt.x, pt.y, w, h, Some(main), None, None, None) else {
        return;
    };
    let square = DWMWCP_DONOTROUND;
    _ = DwmSetWindowAttribute(h, DWMWA_WINDOW_CORNER_PREFERENCE, &square as *const _ as _, 4);
    app(|a| (a.peek, a.peek_pane) = (h, i));
    load_peek();
}

// Asks the worker for the pane's focused item at the picture area's size.
pub(crate) unsafe fn load_peek() {
    let (h, i, dpi) = app(|a| (a.peek, a.peek_pane, a.dpi));
    let Some(fv) = view_of::<IFolderView2>(i) else { return };
    let it = fv.GetItem::<IShellItem>(fv.GetFocusedItem().unwrap_or(-1)).ok();
    let shown = |kind| it.as_ref().and_then(|it| it.GetDisplayName(kind).ok()).map(|p| pwstr(p));
    app(|a| a.peek_name = shown(SIGDN_NORMALDISPLAY).unwrap_or_default());
    let mut rc = RECT::default();
    _ = GetClientRect(h, &mut rc);
    let [pic, ..] = peek_geo(rc, dpi);
    match shown(SIGDN_DESKTOPABSOLUTEPARSING) {
        Some(path) => want_peek(Some(PeekReq { path, w: pic.right - pic.left, h: pic.bottom - pic.top, hwnd: h.0 as isize })),
        None => {
            let mut j = peek_job();
            j.want(None);
            j.done = Some(None);
        }
    }
    _ = InvalidateRect(Some(h), None, false);
}

// Next / previous in the view's own order. Only a real neighbour (S_OK, in range) that got selected moves it.
pub(crate) unsafe fn step_peek(prev: bool) {
    let Some(fv) = view_of::<IFolderView2>(app(|a| a.peek_pane)) else { return };
    let cur = fv.GetFocusedItem().unwrap_or(-1);
    let mut next = -1;
    let hr = (Interface::vtable(&fv).GetVisibleItem)(Interface::as_raw(&fv), cur, prev.into(), &mut next);
    let n = fv.ItemCount(SVGIO_ALLVIEW).unwrap_or(0);
    let flags = SVSI_SELECT.0 | SVSI_DESELECTOTHERS.0 | SVSI_ENSUREVISIBLE.0 | SVSI_FOCUSED.0;
    if hr == S_OK && (0..n).contains(&next) && fv.SelectItem(next, flags as u32).is_ok() {
        load_peek();
    }
    // selecting puts the focus in the list; the preview stays open and keeps the keys, like Quick Look
    let h = app(|a| a.peek);
    if !h.0.is_null() && GetActiveWindow() != h {
        _ = SetActiveWindow(h);
        _ = SetFocus(Some(h));
    }
}

pub(crate) unsafe fn close_peek(h: HWND) {
    let i = app(|a| a.peek_pane);
    _ = DestroyWindow(h);
    focus_view(i);
}

pub(crate) unsafe fn paint_peek(h: HWND) {
    let (dpi, font, small, name) = app(|a| (a.dpi, a.font, a.small_font, a.peek_name.clone()));
    buffered(h, |dc, rc| {
        fill(dc, rc, pal().card);
        frame(dc, rc, 1, pal().line);
        let [pic, name_rc, size_rc] = peek_geo(rc, dpi);
        let j = peek_job();
        match &j.done {
            Some(Some(p)) => {
                // shrunk to fit if the shell handed back something bigger (an icon), never enlarged
                let (pw, ph) = (pic.right - pic.left, pic.bottom - pic.top);
                let k = (pw as f32 / p.w as f32).min(ph as f32 / p.h as f32).min(1.0);
                let (dw, dh) = ((p.w as f32 * k) as i32, (p.h as f32 * k) as i32);
                let (x, y) = ((pic.left + pic.right - dw) / 2, (pic.top + pic.bottom - dh) / 2);
                blit_bgra(dc, RECT { left: x, top: y, right: x + dw, bottom: y + dh }, p.w, p.h, &p.bgra);
                if let Some(b) = p.size {
                    text(dc, size_rc, &human(b), small, pal().sub, DT_CENTER);
                }
            }
            Some(None) => text(dc, pic, tr("No preview", "미리 볼 수 없습니다"), font, pal().sub, DT_CENTER),
            None => text(dc, pic, tr("Loading…", "불러오는 중…"), font, pal().sub, DT_CENTER),
        }
        drop(j);
        text(dc, name_rc, &name, font, pal().text, DT_CENTER);
    });
}

pub(crate) extern "system" fn peek_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => paint_peek(h),
            WM_ERASEBKGND => return LRESULT(1),
            WM_PEEK_DONE => _ = InvalidateRect(Some(h), None, false),
            WM_LBUTTONUP => close_peek(h),
            WM_KEYDOWN => match VIRTUAL_KEY(wp.0 as u16) {
                // a held F3 only opens it; its repeats must not close it again
                VK_ESCAPE | VK_SPACE | VK_F3 if lp.0 & (1 << 30) == 0 => close_peek(h),
                k @ (VK_LEFT | VK_UP | VK_RIGHT | VK_DOWN) => step_peek(k == VK_LEFT || k == VK_UP),
                _ => {}
            },
            // another app or a click behind closes it; stepping takes the activation away only for a moment
            WM_ACTIVATE if (wp.0 & 0xffff) as u32 == WA_INACTIVE => _ = PostMessageW(Some(h), WM_PEEK_LOST, WPARAM(0), LPARAM(0)),
            WM_PEEK_LOST if GetActiveWindow() != h => _ = DestroyWindow(h),
            WM_DESTROY => {
                want_peek(None);
                app(|a| a.peek = HWND::default());
            }
            _ => return DefWindowProcW(h, msg, wp, lp),
        }
        LRESULT(0)
    }
}
