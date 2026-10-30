use crate::*;
use windows::Win32::Storage::FileSystem::{
    GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY,
    FILE_ATTRIBUTE_SYSTEM, INVALID_FILE_ATTRIBUTES,
};

// ---------- folder mark: the toolbar button that gives folders the green-arrow folder icon ----------
// Same mechanism as Explorer's Properties > Customize > Change Icon: an IconResource line in the folder's
// desktop.ini, read by Explorer when the folder is read-only or system. A custom icon the folder already had
// is kept in a [Grids] section and put back on unmark, and so is whether Grids made the folder read-only.

pub(crate) const MARK_ICO: &[u8] = include_bytes!("../folder-mark.ico");
const SHELL: &str = ".ShellClassInfo";
const OWN: &str = "Grids";
const ICON_KEYS: [&str; 3] = ["IconResource", "IconFile", "IconIndex"];
const ICON_TAIL: &str = "\\grids\\folder-mark.ico";

// desktop.ini as sections of raw lines; the first, unnamed one holds anything before the first header.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Ini(Vec<(String, Vec<String>)>);

impl Ini {
    pub(crate) fn parse(text: &str) -> Ini {
        let mut v = vec![(String::new(), Vec::new())];
        for raw in text.split('\n') {
            let raw = raw.trim_end_matches('\r');
            let t = raw.trim();
            if t.len() >= 2 && t.starts_with('[') && t.ends_with(']') {
                v.push((t[1..t.len() - 1].trim().to_string(), Vec::new()));
            } else {
                v.last_mut().unwrap().1.push(raw.to_string());
            }
        }
        Ini(v)
    }

    pub(crate) fn render(&self) -> String {
        let mut out = String::new();
        for (name, lines) in &self.0 {
            let n = lines.iter().rposition(|l| !l.trim().is_empty()).map_or(0, |k| k + 1);
            if name.is_empty() && n == 0 {
                continue;
            }
            if !name.is_empty() {
                out.push_str(&format!("[{name}]\r\n"));
            }
            for l in &lines[..n] {
                out.push_str(l);
                out.push_str("\r\n");
            }
        }
        out
    }

    pub(crate) fn is_blank(&self) -> bool {
        self.0.iter().all(|(_, lines)| lines.iter().all(|l| l.trim().is_empty()))
    }

    fn section(&self, name: &str) -> Option<usize> {
        self.0.iter().position(|(n, _)| n.eq_ignore_ascii_case(name))
    }

    fn key_at(lines: &[String], key: &str) -> Option<usize> {
        lines.iter().position(|l| l.split_once('=').is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(key)))
    }

    pub(crate) fn get(&self, sec: &str, key: &str) -> Option<String> {
        let lines = &self.0[self.section(sec)?].1;
        let l = &lines[Self::key_at(lines, key)?];
        l.split_once('=').map(|(_, v)| v.trim().to_string())
    }

    pub(crate) fn set(&mut self, sec: &str, key: &str, value: &str) {
        let s = match self.section(sec) {
            Some(s) => s,
            None => {
                self.0.push((sec.to_string(), Vec::new()));
                self.0.len() - 1
            }
        };
        let lines = &mut self.0[s].1;
        let line = format!("{key}={value}");
        match Self::key_at(lines, key) {
            Some(k) => lines[k] = line,
            None => {
                let at = lines.iter().rposition(|l| !l.trim().is_empty()).map_or(0, |k| k + 1);
                lines.insert(at, line);
            }
        }
    }

    // Drops the section too once nothing is left in it.
    pub(crate) fn remove(&mut self, sec: &str, key: &str) {
        let Some(s) = self.section(sec) else { return };
        while let Some(k) = Self::key_at(&self.0[s].1, key) {
            self.0[s].1.remove(k);
        }
        if !self.0[s].0.is_empty() && self.0[s].1.iter().all(|l| l.trim().is_empty()) {
            self.0.remove(s);
        }
    }

    pub(crate) fn remove_section(&mut self, sec: &str) {
        self.0.retain(|(n, _)| !n.eq_ignore_ascii_case(sec));
    }
}

pub(crate) fn is_our_icon(resource: Option<&str>) -> bool {
    resource.is_some_and(|r| r.to_ascii_lowercase().contains(ICON_TAIL))
}

pub(crate) fn ini_marked(text: Option<&str>) -> bool {
    text.is_some_and(|t| is_our_icon(Ini::parse(t).get(SHELL, "IconResource").as_deref()))
}

// The desktop.ini text with our icon on; `add_ro`: the folder was neither read-only nor system, so marking
// makes it read-only and unmarking takes that off again.
pub(crate) fn mark_ini(old: Option<&str>, icon: &str, add_ro: bool) -> String {
    let mut ini = Ini::parse(old.unwrap_or(""));
    let previous: Vec<(String, String)> =
        ICON_KEYS.iter().filter_map(|k| ini.get(SHELL, k).map(|v| (format!("Previous{k}"), v))).collect();
    for k in ICON_KEYS {
        ini.remove(SHELL, k);
    }
    ini.remove_section(OWN);
    ini.set(SHELL, "IconResource", &format!("{icon},0"));
    for (k, v) in &previous {
        ini.set(OWN, k, v);
    }
    if add_ro {
        ini.set(OWN, "AddedReadOnly", "1");
    }
    ini.render()
}

// (new desktop.ini text, None = delete the file; take the read-only attribute off the folder)
pub(crate) fn unmark_ini(old: &str) -> (Option<String>, bool) {
    let mut ini = Ini::parse(old);
    for k in ICON_KEYS {
        ini.remove(SHELL, k);
    }
    for k in ICON_KEYS {
        if let Some(v) = ini.get(OWN, &format!("Previous{k}")) {
            ini.set(SHELL, k, &v);
        }
    }
    let added_ro = ini.get(OWN, "AddedReadOnly").as_deref() == Some("1");
    ini.remove_section(OWN);
    if ini.is_blank() {
        (None, added_ro)
    } else {
        (Some(ini.render()), false)
    }
}

// ---------- file system side ----------

fn icon_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Grids").join("folder-mark.ico"))
}

// Outside the exe on purpose: desktop.ini keeps pointing at this file when grids.exe moves or is replaced.
fn ensure_icon() -> std::io::Result<String> {
    let p = icon_path().ok_or_else(|| std::io::Error::other("LOCALAPPDATA is not set"))?;
    if std::fs::metadata(&p).map(|m| m.len()).ok() != Some(MARK_ICO.len() as u64) {
        std::fs::create_dir_all(p.parent().unwrap())?;
        std::fs::write(&p, MARK_ICO)?;
    }
    Ok(p.to_string_lossy().into_owned())
}

fn decode(b: &[u8]) -> String {
    match b {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, false),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => ansi(b, acp()),
    }
}

#[cfg(windows)]
fn acp() -> u32 {
    unsafe { windows::Win32::Globalization::GetACP() }
}
#[cfg(not(windows))]
fn acp() -> u32 {
    65001
}

fn ini_file(dir: &str) -> PathBuf {
    Path::new(dir).join("desktop.ini")
}

fn read_desktop_ini(dir: &str) -> std::io::Result<Option<String>> {
    match std::fs::read(ini_file(dir)) {
        Ok(b) => Ok(Some(decode(&b))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

unsafe fn attrs(path: &Path) -> Option<u32> {
    let a = GetFileAttributesW(&HSTRING::from(path.as_os_str()));
    (a != INVALID_FILE_ATTRIBUTES).then_some(a)
}

unsafe fn set_attrs(path: &Path, a: u32) -> std::io::Result<()> {
    SetFileAttributesW(&HSTRING::from(path.as_os_str()), FILE_FLAGS_AND_ATTRIBUTES(a)).map_err(|e| std::io::Error::other(e.message()))
}

// UTF-16 with a BOM, so a non-ASCII icon path (a Korean user name) survives. A hidden or system file
// can't be overwritten in place, hence the attributes go off first.
unsafe fn put_desktop_ini(dir: &str, text: &str) -> std::io::Result<()> {
    let f = ini_file(dir);
    if attrs(&f).is_some() {
        set_attrs(&f, FILE_ATTRIBUTE_NORMAL.0)?;
    }
    let bytes: Vec<u8> = [0xFFu8, 0xFE].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect();
    std::fs::write(&f, bytes)?;
    set_attrs(&f, FILE_ATTRIBUTE_HIDDEN.0 | FILE_ATTRIBUTE_SYSTEM.0)
}

unsafe fn notify(dir: &str) {
    let w = HSTRING::from(dir);
    let flags = SHCNF_PATHW | SHCNF_FLUSHNOWAIT;
    SHChangeNotify(SHCNE_UPDATEITEM, flags, Some(w.as_ptr() as _), None);
    SHChangeNotify(SHCNE_ATTRIBUTES, flags, Some(w.as_ptr() as _), None);
    if let Some(parent) = Path::new(dir.trim_end_matches('\\')).parent() {
        let p = HSTRING::from(parent.as_os_str());
        SHChangeNotify(SHCNE_UPDATEDIR, flags, Some(p.as_ptr() as _), None);
    }
}

pub(crate) fn is_marked(dir: &str) -> bool {
    ini_marked(read_desktop_ini(dir).ok().flatten().as_deref())
}

unsafe fn mark(dir: &str, icon: &str) -> std::io::Result<()> {
    let old = read_desktop_ini(dir)?;
    let a = attrs(Path::new(dir)).ok_or_else(|| std::io::Error::other("folder unavailable"))?;
    let add_ro = a & (FILE_ATTRIBUTE_READONLY.0 | FILE_ATTRIBUTE_SYSTEM.0) == 0;
    put_desktop_ini(dir, &mark_ini(old.as_deref(), icon, add_ro))?;
    if add_ro {
        set_attrs(Path::new(dir), a | FILE_ATTRIBUTE_READONLY.0)?;
    }
    notify(dir);
    Ok(())
}

unsafe fn unmark(dir: &str) -> std::io::Result<()> {
    let Some(old) = read_desktop_ini(dir)? else { return Ok(()) };
    match unmark_ini(&old) {
        (Some(text), _) => put_desktop_ini(dir, &text)?,
        (None, drop_ro) => {
            let f = ini_file(dir);
            set_attrs(&f, FILE_ATTRIBUTE_NORMAL.0)?;
            std::fs::remove_file(&f)?;
            if let (true, Some(a)) = (drop_ro, attrs(Path::new(dir))) {
                set_attrs(Path::new(dir), a & !FILE_ATTRIBUTE_READONLY.0)?;
            }
        }
    }
    notify(dir);
    Ok(())
}

// Toolbar button: the selected folders in the active pane (no selection: its own folder). All already
// marked -> back to their own icons; otherwise the unmarked ones get the green arrow.
pub(crate) unsafe fn toggle_mark() {
    let i = app(|a| a.s.active);
    let Some(fv) = view_of::<IFolderView2>(i) else { return };
    let path = |it: IShellItem| it.GetDisplayName(SIGDN_FILESYSPATH).ok().map(|p| pwstr(p));
    let picked = fv.ItemCount(SVGIO_SELECTION).unwrap_or(0) > 0;
    let dirs: Vec<String> = if picked {
        let Ok(arr) = fv.GetSelection(false) else { return };
        (0..arr.GetCount().unwrap_or(0)).filter_map(|k| path(arr.GetItemAt(k).ok()?)).filter(|p| Path::new(p).is_dir()).collect()
    } else {
        fv.GetFolder::<IShellItem>().ok().and_then(path).filter(|p| Path::new(p).is_dir()).into_iter().collect()
    };
    if dirs.is_empty() {
        _ = MessageBeep(MB_ICONWARNING);
        return set_note(tr("Select a folder to change its icon", "아이콘을 바꿀 폴더를 선택하세요").into());
    }
    let icon = match ensure_icon() {
        Ok(p) => p,
        Err(e) => return set_note(format!("{}: {e}", tr("Could not change the folder icon", "폴더 아이콘을 바꾸지 못했습니다"))),
    };
    let off = dirs.iter().all(|d| is_marked(d));
    let mut failed: Option<String> = None;
    for d in &dirs {
        let r = if off { unmark(d) } else if is_marked(d) { Ok(()) } else { mark(d, &icon) };
        if let Err(e) = r {
            failed.get_or_insert(format!("{d}: {e}"));
        }
    }
    let n = dirs.len();
    let note = match (failed, KO.load(Ordering::Relaxed), off) {
        (Some(e), true, _) => format!("폴더 아이콘을 바꾸지 못했습니다 — {e}"),
        (Some(e), false, _) => format!("Could not change the folder icon — {e}"),
        (None, true, false) => format!("폴더 {n}개 아이콘을 초록 화살표로 바꿈"),
        (None, true, true) => format!("폴더 {n}개 아이콘을 원래대로 되돌림"),
        (None, false, false) => format!("Marked {n} folder(s) with the green arrow"),
        (None, false, true) => format!("Restored the icon of {n} folder(s)"),
    };
    set_note(note);
}

// ---------- the button's picture ----------

// The .ico frame nearest to `size` (smallest one at least that big), as bytes CreateIconFromResourceEx takes.
pub(crate) fn ico_frame(ico: &[u8], size: i32) -> Option<&[u8]> {
    let count = u16::from_le_bytes([*ico.get(4)?, *ico.get(5)?]) as usize;
    let mut best: Option<(i32, &[u8])> = None;
    for k in 0..count {
        let e = ico.get(6 + 16 * k..22 + 16 * k)?;
        let dim = if e[0] == 0 { 256 } else { e[0] as i32 };
        let len = u32::from_le_bytes(e[8..12].try_into().ok()?) as usize;
        let off = u32::from_le_bytes(e[12..16].try_into().ok()?) as usize;
        let data = ico.get(off..off + len)?;
        let better = match best {
            None => true,
            Some((b, _)) => (dim >= size && (b < size || dim < b)) || (b < size && dim > b),
        };
        if better {
            best = Some((dim, data));
        }
    }
    best.map(|(_, d)| d)
}

thread_local! {
    static MARK_ICON: Cell<(i32, isize)> = const { Cell::new((0, 0)) }; // (size, HICON) last made
}

pub(crate) unsafe fn draw_mark(dc: HDC, rc: RECT, dpi: i32) {
    let size = 18 * dpi / 96;
    let (have, h) = MARK_ICON.with(Cell::get);
    let icon = if have == size && h != 0 {
        HICON(h as _)
    } else {
        let Some(icon) = ico_frame(MARK_ICO, size).and_then(|f| CreateIconFromResourceEx(f, true, 0x0003_0000, size, size, LR_DEFAULTCOLOR).ok())
        else {
            return;
        };
        if h != 0 {
            _ = DestroyIcon(HICON(h as _));
        }
        MARK_ICON.with(|c| c.set((size, icon.0 as isize)));
        icon
    };
    let (x, y) = ((rc.left + rc.right - size) / 2, (rc.top + rc.bottom - size) / 2);
    _ = DrawIconEx(dc, x, y, icon, size, size, 0, None, DI_NORMAL);
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICON: &str = "C:\\Users\\kim\\AppData\\Local\\Grids\\folder-mark.ico";

    #[test]
    fn mark_then_unmark_with_no_desktop_ini_leaves_nothing() {
        let marked = mark_ini(None, ICON, true);
        assert!(ini_marked(Some(&marked)));
        assert_eq!(Ini::parse(&marked).get(SHELL, "IconResource").as_deref(), Some(&*format!("{ICON},0")));
        assert_eq!(unmark_ini(&marked), (None, true));
    }

    #[test]
    fn unmark_restores_a_previous_icon_and_keeps_other_lines() {
        let old = "[.ShellClassInfo]\r\nLocalizedResourceName=@shell32.dll,-21770\r\nIconResource=C:\\x.ico,3\r\n[ViewState]\r\nMode=\r\n";
        let marked = mark_ini(Some(old), ICON, false);
        assert!(ini_marked(Some(&marked)));
        let (back, drop_ro) = unmark_ini(&marked);
        let back = Ini::parse(back.as_deref().unwrap());
        assert!(!drop_ro);
        assert_eq!(back.get(SHELL, "IconResource").as_deref(), Some("C:\\x.ico,3"));
        assert_eq!(back.get(SHELL, "LocalizedResourceName").as_deref(), Some("@shell32.dll,-21770"));
        assert_eq!(back.get("ViewState", "Mode").as_deref(), Some(""));
        assert_eq!(back.get(OWN, "PreviousIconResource"), None);
    }

    #[test]
    fn someone_elses_icon_is_not_ours() {
        assert!(!ini_marked(Some("[.ShellClassInfo]\r\nIconResource=C:\\x.ico,0\r\n")));
        assert!(!ini_marked(None));
    }

    #[test]
    fn picks_the_nearest_frame() {
        let small = ico_frame(MARK_ICO, 18).unwrap();
        let big = ico_frame(MARK_ICO, 300).unwrap();
        assert!(small.len() < big.len());
        assert!(ico_frame(&[0, 0, 1, 0], 16).is_none());
    }
}
