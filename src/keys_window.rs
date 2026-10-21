use crate::*;

// ---------- keyboard shortcuts window (Ctrl+/ or the view menu) ----------

pub(crate) struct KeyRow {
    pub(crate) keys: Vec<String>, // keycaps; in the mouse section, one plain name
    pub(crate) desc: String,
}

// KEYS_EN / KEYS_KO split into [keyboard, mouse & buttons]. Keys break only at ", " and " / ",
// so "Ctrl+C/X/V" and "Ctrl+/" stay one cap; a mouse line without ": " continues the one above.
pub(crate) fn key_rows(text: &str) -> [Vec<KeyRow>; 2] {
    let (keys, mouse) = text.split_once("\n\n").unwrap_or((text, ""));
    let kb = keys
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(k, d)| KeyRow { keys: k.split(", ").flat_map(|p| p.split(" / ")).map(String::from).collect(), desc: d.into() })
        .collect();
    let mut ms: Vec<KeyRow> = Vec::new();
    for l in mouse.lines() {
        match (l.split_once(": "), ms.last_mut()) {
            (Some((k, d)), _) => ms.push(KeyRow { keys: vec![k.into()], desc: d.into() }),
            (None, Some(last)) => {
                last.desc.push('\n');
                last.desc.push_str(l);
            }
            (None, None) => {}
        }
    }
    [kb, ms]
}

pub(crate) struct KeyGeoRow {
    pub(crate) sec: usize,
    pub(crate) row: RECT,
    pub(crate) keys: Vec<RECT>, // keycaps, or the text of a mouse-section name
    pub(crate) desc: RECT,      // description text
}

pub(crate) struct KeysGeo {
    pub(crate) h: i32,    // content height (below the fixed title bar)
    pub(crate) titles: [RECT; 2],
    pub(crate) rows: Vec<KeyGeoRow>,
}

pub(crate) const KEYS_PAD: i32 = 20;

// Content layout in px. measure(text, max_w, cap): a cap's one-line size, or text wrapped at max_w.
// Painting and the window size both use this, so what was measured is what gets drawn.
pub(crate) fn keys_layout(secs: &[Vec<KeyRow>; 2], width: i32, dpi: i32, measure: &dyn Fn(&str, i32, bool) -> (i32, i32)) -> KeysGeo {
    let s = |v: i32| v * dpi / 96;
    let (pad, gap, cx, cy, cgap) = (s(KEYS_PAD), s(16), s(7), s(3), s(5));
    let inner = (width - 2 * pad).max(s(80));
    let lh = measure("Ag", inner, true).1;
    let cap_w = |t: &str| measure(t, inner, true).0 + 2 * cx;
    let (mut one, mut group) = (0, 0);
    for r in &secs[0] {
        let caps: Vec<i32> = r.keys.iter().map(|k| cap_w(k)).collect();
        one = one.max(caps.iter().copied().max().unwrap_or(0));
        group = group.max(caps.iter().sum::<i32>() + cgap * (caps.len() as i32 - 1).max(0));
    }
    // widest single cap, or a whole row of caps up to 40%; never over half (a wider cap gets an ellipsis)
    let key_w = one.max(group.min(inner * 2 / 5)).min(inner / 2);
    let (dx, dw, ch) = (pad + key_w + gap, inner - key_w - gap, lh + 2 * cy);
    let mut y = pad;
    let mut titles = [RECT::default(); 2];
    let mut rows = Vec::new();
    for (sec, list) in secs.iter().enumerate() {
        if sec > 0 {
            y += s(14);
        }
        titles[sec] = RECT { left: pad, top: y, right: pad + inner, bottom: y + lh };
        y += lh + s(6);
        for r in list {
            let mut keys = Vec::new();
            if sec == 0 {
                let (mut x, mut line) = (0, 0);
                for k in &r.keys {
                    let w = cap_w(k).min(key_w);
                    if x > 0 && x + w > key_w {
                        (x, line) = (0, line + 1);
                    }
                    let top = y + line * (ch + cgap);
                    keys.push(RECT { left: pad + x, top, right: pad + x + w, bottom: top + ch });
                    x += w + cgap;
                }
            } else {
                let h = measure(&r.keys[0], key_w, false).1;
                keys.push(RECT { left: pad, top: y + cy, right: pad + key_w, bottom: y + cy + h });
            }
            let dh = measure(&r.desc, dw, false).1;
            let desc = RECT { left: dx, top: y + cy, right: dx + dw, bottom: y + cy + dh };
            let bottom = keys.iter().map(|k| k.bottom + if sec == 0 { 0 } else { cy }).chain([desc.bottom + cy]).max().unwrap_or(y);
            rows.push(KeyGeoRow { sec, row: RECT { left: pad, top: y, right: pad + inner, bottom }, keys, desc });
            y = bottom + s(6);
        }
    }
    KeysGeo { h: y - s(6) + pad, titles, rows }
}

pub(crate) unsafe fn keys_geo(width: i32, dpi: i32) -> KeysGeo {
    let font = app(|a| a.font);
    let dc = GetDC(None);
    let measure = |t: &str, max_w: i32, cap: bool| {
        if !cap {
            return wrap_text(dc, RECT { right: max_w, ..Default::default() }, t, font, 0, true);
        }
        let old = SelectObject(dc, font.into());
        let mut sz = SIZE::default();
        _ = GetTextExtentPoint32W(dc, &t.encode_utf16().collect::<Vec<_>>(), &mut sz);
        SelectObject(dc, old);
        (sz.cx, sz.cy)
    };
    let g = keys_layout(&key_rows(tr(KEYS_EN, KEYS_KO)), width, dpi, &measure);
    ReleaseDC(None, dc);
    g
}

pub(crate) fn keys_head(dpi: i32) -> i32 {
    44 * dpi / 96
}

pub(crate) fn keys_close(width: i32, dpi: i32) -> RECT {
    let s = |v: i32| v * dpi / 96;
    RECT { left: width - s(44), top: s(6), right: width - s(12), bottom: s(38) }
}

pub(crate) unsafe fn open_keys() {
    let (main, dpi, open) = app(|a| (a.main, a.dpi, a.keys_popup));
    if !open.0.is_null() {
        _ = SetFocus(Some(open));
        return;
    }
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    _ = GetMonitorInfoW(MonitorFromWindow(main, MONITOR_DEFAULTTONEAREST), &mut mi);
    let work = mi.rcWork;
    let w = (760 * dpi / 96).min((work.right - work.left) * 9 / 10);
    let h = (keys_head(dpi) + keys_geo(w, dpi).h).min((work.bottom - work.top) * 9 / 10);
    let mut rc = RECT::default();
    _ = GetWindowRect(main, &mut rc);
    let x = ((rc.left + rc.right - w) / 2).clamp(work.left, work.right - w);
    let y = ((rc.top + rc.bottom - h) / 2).clamp(work.top, work.bottom - h);
    let prev = GetFocus();
    app(|a| (a.keys_prev, a.keys_scroll) = (prev, 0));
    let title = HSTRING::from(tr("Keyboard shortcuts", "단축키"));
    let Ok(p) = CreateWindowExW(WS_EX_TOOLWINDOW, w!("GridsKeys"), &title, WS_POPUP | WS_VISIBLE, x, y, w, h, Some(main), None, None, None) else {
        return;
    };
    let square = DWMWCP_DONOTROUND;
    _ = DwmSetWindowAttribute(p, DWMWA_WINDOW_CORNER_PREFERENCE, &square as *const _ as _, 4);
    app(|a| a.keys_popup = p);
}

// Esc, ✕ or Ctrl+/ again: back to where the user was. (Losing activation just closes: focus is already elsewhere.)
pub(crate) unsafe fn close_keys() {
    let (h, prev, active) = app(|a| (a.keys_popup, a.keys_prev, a.s.active));
    _ = DestroyWindow(h);
    if !prev.0.is_null() && IsWindow(Some(prev)).as_bool() {
        _ = SetFocus(Some(prev));
    } else {
        focus_view(active);
    }
}

pub(crate) unsafe fn toggle_keys() {
    if app(|a| a.keys_popup.0.is_null()) {
        open_keys();
    } else {
        close_keys();
    }
}

pub(crate) unsafe fn keys_scroll_to(h: HWND, to: impl FnOnce(i32, i32) -> i32) {
    let mut rc = RECT::default();
    _ = GetClientRect(h, &mut rc);
    let dpi = app(|a| a.dpi);
    let page = rc.bottom - keys_head(dpi);
    let max = (keys_geo(rc.right, dpi).h - page).max(0);
    if app(|a| {
        let y = to(a.keys_scroll, page).clamp(0, max);
        std::mem::replace(&mut a.keys_scroll, y) != y
    }) {
        _ = InvalidateRect(Some(h), None, false);
    }
}

pub(crate) unsafe fn paint_keys(h: HWND) {
    let (dpi, font, small, icon_font, scroll) = app(|a| (a.dpi, a.font, a.small_font, a.icon_font, a.keys_scroll));
    let mut rc = RECT::default();
    _ = GetClientRect(h, &mut rc);
    let g = keys_geo(rc.right, dpi);
    let rows = key_rows(tr(KEYS_EN, KEYS_KO));
    let s = |v: i32| v * dpi / 96;
    let head = keys_head(dpi);
    let down = |r: RECT| RECT { top: r.top + head - scroll, bottom: r.bottom + head - scroll, ..r };
    buffered(h, |dc, rc| {
        fill(dc, rc, pal().card);
        let names = [tr("Keyboard", "키보드"), tr("Mouse & buttons", "마우스·버튼")];
        for (t, r) in names.iter().zip(g.titles) {
            text(dc, down(r), t, small, pal().sub, DT_LEFT);
        }
        for (gr, r) in g.rows.iter().zip(rows.iter().flatten()) {
            let row = down(gr.row);
            if row.bottom < head || row.top > rc.bottom {
                continue; // scrolled out of view
            }
            for (k, kr) in r.keys.iter().zip(&gr.keys) {
                let kr = down(*kr);
                if gr.sec == 0 {
                    fill(dc, kr, pal().pill);
                    frame(dc, kr, 1, pal().line);
                    text(dc, RECT { left: kr.left + s(4), right: kr.right - s(4), ..kr }, k, font, pal().text, DT_CENTER);
                } else {
                    wrap_text(dc, kr, k, font, pal().text, false);
                }
            }
            wrap_text(dc, down(gr.desc), &r.desc, font, pal().text, false);
        }
        // fixed title bar over the scrolled content
        let bar = RECT { bottom: head, ..rc };
        fill(dc, bar, pal().card);
        fill(dc, RECT { top: head - 1, ..bar }, pal().line);
        text(dc, RECT { left: s(KEYS_PAD), ..bar }, tr("Keyboard shortcuts", "단축키"), font, pal().text, DT_LEFT);
        text(dc, keys_close(rc.right, dpi), "\u{E711}", icon_font, pal().sub, DT_CENTER);
        let page = rc.bottom - head;
        if g.h > page {
            let th = (page * page / g.h).max(s(24));
            let top = head + scroll * (page - th) / (g.h - page);
            fill(dc, RECT { left: rc.right - s(6), top, right: rc.right - s(3), bottom: top + th }, pal().sub);
        }
        frame(dc, rc, 1, pal().line);
    });
}

pub(crate) extern "system" fn keys_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let dpi = app(|a| a.dpi);
        let s = |v: i32| v * dpi / 96; // not app() inside: keys_scroll_to calls these while APP is borrowed
        match msg {
            WM_PAINT => paint_keys(h),
            WM_ERASEBKGND => return LRESULT(1),
            WM_MOUSEWHEEL => keys_scroll_to(h, |y, _| y - s((wp.0 >> 16) as i16 as i32)), // 120 px a notch
            WM_LBUTTONUP => {
                let (x, y) = xy(lp);
                let mut rc = RECT::default();
                _ = GetClientRect(h, &mut rc);
                let r = keys_close(rc.right, dpi);
                if x >= r.left && x < r.right && y >= r.top && y < r.bottom {
                    close_keys();
                }
            }
            // reaches here because key() leaves these to the focused window
            WM_KEYDOWN => match VIRTUAL_KEY(wp.0 as u16) {
                VK_ESCAPE => close_keys(),
                VK_UP => keys_scroll_to(h, |y, _| y - s(40)),
                VK_DOWN => keys_scroll_to(h, |y, _| y + s(40)),
                VK_PRIOR => keys_scroll_to(h, |y, p| y - p * 9 / 10),
                VK_NEXT => keys_scroll_to(h, |y, p| y + p * 9 / 10),
                VK_HOME => keys_scroll_to(h, |_, _| 0),
                VK_END => keys_scroll_to(h, |_, _| i32::MAX),
                _ => {}
            },
            WM_ACTIVATE if (wp.0 & 0xffff) as u32 == WA_INACTIVE => _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0)),
            WM_DESTROY => app(|a| a.keys_popup = HWND::default()),
            _ => return DefWindowProcW(h, msg, wp, lp),
        }
        LRESULT(0)
    }
}
