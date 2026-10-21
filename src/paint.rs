use crate::*;

// ---------- painting ----------

// Ground, the sidebar plate and the status line. Pane plates are child windows (paint_pane).
pub(crate) unsafe fn paint_main(h: HWND) {
    let (g, status, font, dpi) = app(|a| (geo(a), a.status.clone(), a.font, a.dpi));
    buffered(h, |dc, rc| {
        fill(dc, rc, pal().bg);
        if g.tree {
            let side = side_rect(&g);
            fill(dc, side, pal().card);
            frame(dc, side, 1, pal().line);
        }
        let st = status_rect(&g);
        text(dc, RECT { left: g.pad + 4 * dpi / 96, ..st }, &status, font, pal().sub, DT_LEFT);
    });
}

#[derive(Clone, PartialEq)]
pub(crate) enum Crumb {
    Sep,
    Here,
    Go(String),
    More(Vec<(String, String)>), // folded parents, nearest first
}

// The folder icon's square at the front of the address pill, like Explorer's address bar; none in a narrow pill.
// Fixed by the geometry alone, so crumbs and edit never shift when an icon arrives or goes.
pub(crate) fn addr_icon(addr: RECT, dpi: i32) -> Option<RECT> {
    let s = |v: i32| v * dpi / 96;
    if addr.right - addr.left < s(90) {
        return None;
    }
    let (left, top) = (addr.left + s(6), addr.top + (addr.bottom - addr.top - s(16)) / 2);
    Some(RECT { left, top, right: left + s(16), bottom: top + s(16) })
}

// Where crumbs go in the address pill: the edit's horizontal span, a little inside the pill vertically.
pub(crate) fn crumb_field(addr: RECT, dpi: i32) -> RECT {
    let s = |v: i32| v * dpi / 96;
    let left = addr.left + if addr_icon(addr, dpi).is_some() { s(26) } else { s(8) };
    RECT { left, top: addr.top + s(3), right: (addr.right - s(6)).max(left), bottom: addr.bottom - s(3) }
}

// Pane i's crumbs laid out in `field`: (rect, label, target). Painting and mouse hits both go through here.
pub(crate) unsafe fn crumb_layout(dc: HDC, font: HFONT, i: usize, field: RECT) -> Vec<(RECT, String, Crumb)> {
    let Some((parse, disp, dpi)) = app(|a| Some((a.s.paths[i].clone(), a.panes[i].as_ref()?.disp.clone(), a.dpi))) else {
        return Vec::new();
    };
    let s = |v: i32| v * dpi / 96;
    let mut items = crumbs(&parse);
    if items.is_empty() {
        items.push((disp, String::new())); // a shell place: its name, to click into the edit
    }
    let old = SelectObject(dc, font.into());
    let width = |t: &str| {
        let w: Vec<u16> = t.encode_utf16().collect();
        let mut sz = SIZE::default();
        _ = GetTextExtentPoint32W(dc, &w, &mut sz);
        sz.cx
    };
    let w: Vec<i32> = items.iter().map(|(n, _)| width(n) + 2 * s(4)).collect();
    let (sep, ell) = (width("›") + 2 * s(2), width("…") + 2 * s(4));
    SelectObject(dc, old);
    let (first, more, last) = crumb_fit(&w, field.right - field.left, sep, ell, s(16));
    let mut out = Vec::new();
    let mut x = field.left;
    let mut put = |wd: i32, label: &str, c: Crumb| {
        out.push((RECT { left: x, right: x + wd, ..field }, label.to_string(), c));
        x += wd;
    };
    if more {
        put(ell, "…", Crumb::More(items[..first].iter().rev().cloned().collect()));
        put(sep, "›", Crumb::Sep);
    }
    for k in first..items.len() {
        if k + 1 == items.len() {
            put(last, &items[k].0, Crumb::Here);
        } else {
            put(w[k], &items[k].0, Crumb::Go(items[k].1.clone()));
            put(sep, "›", Crumb::Sep);
        }
    }
    out.retain_mut(|(r, ..)| {
        (r.left, r.right) = (r.left.max(field.left), r.right.min(field.right));
        r.right > r.left
    });
    out
}

pub(crate) fn addr_rect(h: HWND, dpi: i32) -> RECT {
    let mut rc = RECT::default();
    _ = unsafe { GetClientRect(h, &mut rc) };
    pane_geo(rc.right, rc.bottom, dpi).addr
}

// The crumb under a pane-client point: None off the crumbs (edit showing, or outside the address pill),
// Some(None) on the pill's empty space.
pub(crate) unsafe fn crumb_at(h: HWND, i: usize, x: i32, y: i32) -> Option<Option<(usize, RECT, Crumb)>> {
    if GetDlgItem(Some(h), ID_EDIT).is_ok_and(|e| IsWindowVisible(e).as_bool()) {
        return None;
    }
    let (dpi, font) = app(|a| (a.dpi, a.font));
    let inr = |r: &RECT| x >= r.left && x < r.right && y >= r.top && y < r.bottom;
    let addr = addr_rect(h, dpi);
    if !inr(&addr) {
        return None;
    }
    let dc = GetDC(Some(h));
    let items = crumb_layout(dc, font, i, crumb_field(addr, dpi));
    ReleaseDC(Some(h), dc);
    Some(items.into_iter().enumerate().find(|(_, (r, _, c))| *c != Crumb::Sep && inr(r)).map(|(k, (r, _, c))| (k, r, c)))
}

// The address edit shows only while in use (focused, or holding an edit); otherwise the pill draws crumbs.
pub(crate) unsafe fn sync_addr(h: HWND) {
    let Ok(e) = GetDlgItem(Some(h), ID_EDIT) else { return };
    let disp = app(|a| {
        if a.crumb_hot.is_some_and(|(w, _)| w == h) {
            a.crumb_hot = None; // the crumbs change under it
        }
        a.panes.iter().flatten().find(|p| p.hwnd == h).map(|p| p.disp.clone())
    });
    let show = GetFocus() == e || text_of(e) != disp.unwrap_or_default();
    _ = ShowWindow(e, if show { SW_SHOWNA } else { SW_HIDE });
    _ = InvalidateRect(Some(h), None, false);
}

// Click on the address pill: a parent crumb goes there, '…' lists the folded parents, the rest starts typing.
pub(crate) unsafe fn crumb_click(h: HWND, i: usize, x: i32, y: i32) {
    let target = match crumb_at(h, i, x, y) {
        None => return,
        Some(Some((_, _, Crumb::Go(p)))) => Some(p),
        Some(Some((_, r, Crumb::More(list)))) => {
            let Ok(menu) = CreatePopupMenu() else { return };
            for (k, (n, _)) in list.iter().enumerate() {
                _ = AppendMenuW(menu, MF_STRING, k + 1, &HSTRING::from(n.replace('&', "&&")));
            }
            let mut p = POINT { x: r.left, y: r.bottom };
            _ = ClientToScreen(h, &mut p);
            let pick = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_LEFTALIGN | TPM_TOPALIGN, p.x, p.y, None, h, None).0;
            _ = DestroyMenu(menu);
            (pick > 0).then(|| list[pick as usize - 1].1.clone())
        }
        Some(_) => {
            focus_ctl(i, ID_EDIT);
            return;
        }
    };
    if target.is_some_and(|p| !navigate(i, &p)) {
        _ = MessageBeep(MB_ICONWARNING); // gone or unreachable
    }
    focus_view(i);
}

pub(crate) unsafe fn crumb_hover(h: HWND, k: Option<usize>) {
    let hot = k.map(|k| (h, k));
    let (old, dpi) = app(|a| (std::mem::replace(&mut a.crumb_hot, hot), a.dpi));
    if old == hot {
        return;
    }
    for w in [old.map(|o| o.0), hot.map(|o| o.0)].into_iter().flatten() {
        _ = InvalidateRect(Some(w), Some(&addr_rect(w, dpi)), false);
    }
    if hot.is_some() && old.is_none_or(|(w, _)| w != h) {
        let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: h, dwHoverTime: 0 };
        _ = TrackMouseEvent(&mut t);
    }
}

// One white plate per pane: a single accent border when active, flat fields in the top bar.
pub(crate) unsafe fn paint_pane(h: HWND) {
    let (active, i, dpi, icon_font, small_font, font, accent, hot, tint, folder) = app(|a| {
        let i = a.panes.iter().position(|p| p.as_ref().is_some_and(|p| p.hwnd == h));
        // a drop target-to-be looks active too
        let active = a.panes[a.s.active].as_ref().is_some_and(|p| p.hwnd == h) || a.drop_hover == h;
        let hot = a.crumb_hot.filter(|(w, _)| *w == h).map(|(_, k)| k);
        let tint = i.and_then(|i| tint_color(a.s.tints[i]));
        let folder = i.and_then(|i| a.panes[i].as_ref()).map_or(-1, |p| p.icon);
        (active, i, a.dpi, a.icon_font, a.small_font, a.font, a.s.accent_now(), hot, tint, folder)
    });
    let ilist = match app(|a| a.ilist.clone()) {
        None if folder >= 0 => {
            let l = SHGetImageList::<IImageList>(SHIL_SMALL as i32).ok();
            app(|a| a.ilist = l.clone());
            l
        }
        l => l,
    };
    let crumbs_on = GetDlgItem(Some(h), ID_EDIT).is_ok_and(|e| !IsWindowVisible(e).as_bool());
    let filtered = i.is_some_and(|i| FILTERS.lock().is_ok_and(|f| !f[i].is_empty()));
    let (head, ink) = header_colors(h);
    let focus = GetFocus();
    let focused = |id: i32| GetDlgItem(Some(h), id).is_ok_and(|e| e == focus);
    let (addr_on, filt_on) = (focused(ID_EDIT), focused(ID_FILTER));
    buffered(h, |dc, rc| {
        let s = |v: i32| v * dpi / 96;
        let pg = pane_geo(rc.right, rc.bottom, dpi);
        fill(dc, rc, pal().card);
        // active: header bar in the selection colour, same colour as the border
        fill(dc, RECT { bottom: pg.view.top, ..rc }, head);
        if active {
            frame(dc, rc, s(2), accent);
        } else {
            frame(dc, rc, 1, pal().line);
            fill(dc, RECT { left: pg.view.left, top: pg.view.top - 1, right: pg.view.right, bottom: pg.view.top }, pal().line);
        }
        // colour label: a strip above the buttons, and the number on a chip of that colour
        if let Some(t) = tint {
            fill(dc, tint_band(rc.right, dpi, active), t);
        }
        if let Some(i) = i.filter(|_| pg.num.right > pg.num.left) {
            let ink = match tint {
                Some(t) => {
                    fill(dc, pg.num, t);
                    on_color(t)
                }
                None if active => ink,
                None => pal().sub,
            };
            text(dc, pg.num, &(i + 1).to_string(), small_font, ink, DT_CENTER);
        }
        for (p, on) in [(pg.addr, addr_on), (pg.filt, filt_on)] {
            fill(dc, p, pal().pill);
            if on {
                frame(dc, p, 1, if active { ink } else { accent });
            }
        }
        if let (Some(il), Some(r), true) = (&ilist, addr_icon(pg.addr, dpi), folder >= 0) {
            if let Ok(icon) = il.GetIcon(folder, ILD_NORMAL.0) {
                _ = DrawIconEx(dc, r.left, r.top, icon, r.right - r.left, r.bottom - r.top, 0, None, DI_NORMAL);
                _ = DestroyIcon(icon);
            }
        }
        // idle address bar: C: › Users › Public, parents dimmed, the crumb under the mouse shaded
        if let Some(i) = i.filter(|_| crumbs_on) {
            for (k, (r, label, c)) in crumb_layout(dc, font, i, crumb_field(pg.addr, dpi)).into_iter().enumerate() {
                if c == Crumb::Sep {
                    text(dc, r, &label, font, pal().sub, DT_CENTER);
                    continue;
                }
                if hot == Some(k) {
                    fill(dc, r, blend(pal().pill, pal().text, 0.08));
                }
                let ink = if c == Crumb::Here { pal().text } else { pal().sub };
                text(dc, RECT { left: r.left + s(4), right: r.right - s(4), ..r }, &label, font, ink, DT_LEFT);
            }
        }
        if filtered {
            // filter applied: accent chip left of the edit (which starts at filt.left + s(24))
            let chip =
                RECT { left: pg.filt.left + s(3), top: pg.filt.top + s(3), right: pg.filt.left + s(21), bottom: pg.filt.bottom - s(3) };
            fill(dc, chip, accent);
            text(dc, chip, ICON_FILTER, icon_font, on_color(accent), DT_CENTER);
        } else {
            let icon = RECT { left: pg.filt.left + s(4), right: pg.filt.left + s(22), ..pg.filt };
            text(dc, icon, ICON_SEARCH, icon_font, pal().sub, DT_CENTER);
        }
    });
}
