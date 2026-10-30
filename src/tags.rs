use crate::*;
use std::collections::HashMap;
use windows::Win32::Storage::EnhancedStorage::{PKEY_DateModified, PKEY_Keywords};
use windows::Win32::System::Com::StructuredStorage::{InitPropVariantFromStringVector, PropVariantToStringAlloc};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreFromParsingName, GPS_DEFAULT, GPS_READWRITE};

// ---------- file tags ----------
// Every file and folder can carry tags. They are kept in grids-tags.txt next to grids.exe (portable, like
// grids.ini), so PDF, HWP and anything else can be tagged. Where Windows itself can store tags in the file
// (Office documents, JPEG, MP3 ...), they are also written there, so Explorer's "Tags" column shows them and
// Windows search finds them. The name filter takes #tag to show only files with that tag.

static STORE: Mutex<Option<HashMap<String, (String, Vec<String>)>>> = Mutex::new(None); // lower-case path -> (path, tags)

fn store_file() -> PathBuf {
    ini_path().with_file_name("grids-tags.txt")
}

fn key(path: &str) -> String {
    path.trim_end_matches('\\').to_lowercase()
}

/// "a; b, c ;a" -> ["a", "b", "c"]: split on ; and , trimmed, a leading # dropped, duplicates (any case) once.
pub(crate) fn parse_tags(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in text.split([';', ',']).map(|t| t.trim().trim_start_matches('#').trim()) {
        if !t.is_empty() && !out.iter().any(|o| o.to_lowercase() == t.to_lowercase()) {
            out.push(t.to_string());
        }
    }
    out
}

/// One line per path: "path<TAB>tag;tag". Lines that don't parse are skipped.
pub(crate) fn parse_store(text: &str) -> HashMap<String, (String, Vec<String>)> {
    let mut m = HashMap::new();
    for line in text.lines() {
        if let Some((p, t)) = line.split_once('\t') {
            let tags = parse_tags(t);
            if !p.trim().is_empty() && !tags.is_empty() {
                m.insert(key(p.trim()), (p.trim().to_string(), tags));
            }
        }
    }
    m
}

pub(crate) fn render_store(m: &HashMap<String, (String, Vec<String>)>) -> String {
    let mut lines: Vec<String> = m.values().map(|(p, t)| format!("{p}\t{}", t.join(";"))).collect();
    lines.sort();
    lines.join("\r\n") + "\r\n"
}

fn with_store<R>(f: impl FnOnce(&mut HashMap<String, (String, Vec<String>)>) -> R) -> R {
    let mut g = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let m = g.get_or_insert_with(|| std::fs::read(store_file()).map(|b| parse_store(&String::from_utf8_lossy(&b))).unwrap_or_default());
    f(m)
}

fn save_store() -> std::io::Result<()> {
    let text = with_store(|m| render_store(m));
    let path = store_file();
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)
}

/// True when any query (without its #) is part of any tag, any case.
pub(crate) fn tags_match(tags: &[String], queries: &[String]) -> bool {
    queries.iter().any(|q| {
        let q = q.to_lowercase();
        tags.iter().any(|t| t.to_lowercase().contains(&q))
    })
}

unsafe fn windows_tags(path: &str) -> Vec<String> {
    let Ok(ps) = SHGetPropertyStoreFromParsingName::<_, _, IPropertyStore>(&HSTRING::from(path), None, GPS_DEFAULT) else { return Vec::new() };
    let Ok(v) = ps.GetValue(&PKEY_Keywords) else { return Vec::new() };
    match PropVariantToStringAlloc(&v) {
        Ok(p) => parse_tags(&pwstr(p)),
        Err(_) => Vec::new(),
    }
}

// Windows tags inside the file, for the formats that have them. The file's modified date is put back
// afterwards: tagging is not editing. False when the format can't hold tags (PDF, HWP, TXT ...).
unsafe fn set_windows_tags(path: &str, tags: &[String]) -> bool {
    let Ok(ps) = SHGetPropertyStoreFromParsingName::<_, _, IPropertyStore>(&HSTRING::from(path), None, GPS_READWRITE) else { return false };
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let wide: Vec<HSTRING> = tags.iter().map(HSTRING::from).collect();
    let ptrs: Vec<PCWSTR> = wide.iter().map(|h| PCWSTR(h.as_ptr())).collect();
    let Ok(v) = InitPropVariantFromStringVector(Some(&ptrs)) else { return false };
    let ok = ps.SetValue(&PKEY_Keywords, &v).is_ok() && ps.Commit().is_ok();
    drop(ps);
    if let (true, Some(t)) = (ok, modified) {
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(path) {
            _ = f.set_modified(t);
        }
    }
    ok
}

/// Grids' own tags plus the ones Windows keeps in the file.
pub(crate) unsafe fn all_tags(path: &str) -> Vec<String> {
    let mine = with_store(|m| m.get(&key(path)).map(|e| e.1.clone()).unwrap_or_default());
    let mut all = mine;
    for t in windows_tags(path) {
        if !all.iter().any(|o| o.to_lowercase() == t.to_lowercase()) {
            all.push(t);
        }
    }
    all
}

unsafe fn set_tags(path: &str, tags: &[String]) -> bool {
    with_store(|m| {
        if tags.is_empty() {
            m.remove(&key(path));
        } else {
            m.insert(key(path), (path.to_string(), tags.to_vec()));
        }
    });
    let win = set_windows_tags(path, tags);
    let w = HSTRING::from(path);
    SHChangeNotify(SHCNE_UPDATEITEM, SHCNF_PATHW | SHCNF_FLUSHNOWAIT, Some(w.as_ptr() as _), None);
    win
}

// The "Tags" column right after "Date modified" (Name, Date modified, Tags, Type ...), once per pane.
pub(crate) unsafe fn ensure_tag_column(i: usize) {
    let Some(fv) = view_of::<IFolderView2>(i) else { return };
    let Ok(cm) = fv.cast::<IColumnManager>() else { return };
    let Ok(n) = cm.GetColumnCount(CM_ENUM_VISIBLE) else { return };
    let mut keys = vec![PROPERTYKEY::default(); n as usize];
    if cm.GetColumns(CM_ENUM_VISIBLE, &mut keys).is_err() || keys.iter().any(|k| *k == PKEY_Keywords) {
        return;
    }
    let at = keys.iter().position(|k| *k == PKEY_DateModified).map_or(keys.len().min(1), |p| p + 1);
    keys.insert(at, PKEY_Keywords);
    _ = cm.SetColumns(&keys);
}

// Selected items of pane i with a file system path.
unsafe fn selected_paths(i: usize) -> Vec<String> {
    let Some(fv) = view_of::<IFolderView2>(i) else { return Vec::new() };
    if fv.ItemCount(SVGIO_SELECTION).unwrap_or(0) == 0 {
        return Vec::new();
    }
    let Ok(arr) = fv.GetSelection(false) else { return Vec::new() };
    let path = |it: IShellItem| it.GetDisplayName(SIGDN_FILESYSPATH).ok().map(|p| pwstr(p));
    (0..arr.GetCount().unwrap_or(0)).filter_map(|k| path(arr.GetItemAt(k).ok()?)).collect()
}

// Toolbar tag button / Ctrl+Shift+G: the tag box over the active pane, filled with the first selected item's tags.
pub(crate) unsafe fn open_tags() {
    let i = app(|a| a.s.active);
    let paths = selected_paths(i);
    if paths.is_empty() {
        _ = MessageBeep(MB_ICONWARNING);
        return set_note(tr("Select files to tag", "태그를 붙일 파일을 선택하세요").into());
    }
    let now = all_tags(&paths[0]).join("; ");
    open_box(i, MatchMode::Tag(paths), &now);
}

// Enter in the tag box: those tags on every selected item (empty: tags removed).
pub(crate) unsafe fn apply_tags(i: usize, paths: &[String], text: &str) {
    let tags = parse_tags(text);
    let in_file = paths.iter().filter(|p| set_tags(p, &tags)).count();
    let saved = save_store();
    if !tags.is_empty() {
        ensure_tag_column(i);
    }
    app(|a| a.status_key.clear()); // the status line shows the selected item's tags: read them again
    let (n, ko) = (paths.len(), KO.load(Ordering::Relaxed));
    let note = match (saved, tags.is_empty(), ko) {
        (Err(e), _, true) => format!("태그를 저장하지 못했습니다: {e}"),
        (Err(e), _, false) => format!("Could not save tags: {e}"),
        (Ok(()), true, true) => format!("{n}개 항목의 태그를 지움"),
        (Ok(()), true, false) => format!("Tags removed from {n} item(s)"),
        (Ok(()), false, true) => format!("{n}개 항목에 태그 {} (탐색기 '태그' 열에도 표시: {in_file}개)", tags.join(", ")),
        (Ok(()), false, false) => format!("Tagged {n} item(s): {} (also in Explorer's Tags column: {in_file})", tags.join(", ")),
    };
    set_note(note);
}

/// Status line text for one selected item: "Tags: a, b", or None.
pub(crate) unsafe fn tag_note(path: &str) -> Option<String> {
    let t = all_tags(path);
    (!t.is_empty()).then(|| format!("{}: {}", tr("Tags", "태그"), t.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_parse_and_match() {
        assert_eq!(parse_tags(" 보고서; #급함, 보고서 ;; "), vec!["보고서", "급함"]);
        assert_eq!(parse_tags("A;a;B"), vec!["A", "B"]);
        let t = vec!["보고서".to_string(), "Urgent".to_string()];
        assert!(tags_match(&t, &["보고".into()]));
        assert!(tags_match(&t, &["urg".into()]));
        assert!(!tags_match(&t, &["계약".into()]));
        assert_eq!(filter_spec("#보고; pdf ;#"), "#보고;*pdf*");
    }

    #[test]
    fn store_round_trip() {
        let m = parse_store("C:\\a\\x.pdf\t보고서;급함\r\nbad line\r\nC:\\b\\y.hwp\t\r\n");
        assert_eq!(m.len(), 1);
        assert_eq!(m.get("c:\\a\\x.pdf").unwrap().1, vec!["보고서", "급함"]);
        assert_eq!(parse_store(&render_store(&m)), m);
    }
}
