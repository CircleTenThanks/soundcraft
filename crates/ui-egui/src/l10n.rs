//! 画面に出す英語を、日本語カタログで置き換える。英語のリテラルは呼び出し側に残す。

#[path = "l10n_ja.rs"]
mod l10n_ja;

/// `english` がカタログにあれば日本語を返す。無ければ英語のまま返す。
pub fn tr(english: &str) -> String {
    match lookup(english) {
        Some(ja) => ja.to_string(),
        None => english.to_string(),
    }
}

/// カタログの文にある `{name}` を、渡した値で置き換える。
pub fn tr_fmt(english: &str, vars: &[(&str, &str)]) -> String {
    let mut text = tr(english);
    for (key, value) in vars {
        text = text.replace(&format!("{{{key}}}"), value);
    }
    text
}

/// グリッド値の表示。付点と三連符、秒、フレーム、サンプルを分ける。
pub fn tr_grid(label: &str) -> String {
    if let Some(base) = label.strip_suffix(" triplet") {
        return format!("{}{}", tr(base), tr(" triplet"));
    }
    if let Some(base) = label.strip_suffix(" dotted") {
        return format!("{}{}", tr(base), tr(" dotted"));
    }
    if let Some(n) = label.strip_suffix(" sec") {
        return format!("{n}{}", tr(" sec"));
    }
    if let Some(n) = label.strip_suffix(" frames") {
        return tr_fmt("{n} frames", &[("n", n)]);
    }
    if let Some(n) = label.strip_suffix(" frame") {
        return tr_fmt("{n} frame", &[("n", n)]);
    }
    if let Some(n) = label.strip_suffix(" samples") {
        return tr_fmt("{n} samples", &[("n", n)]);
    }
    tr(label)
}

/// オートメーションレーンの表示。`send A level` のような形を分ける。
pub fn tr_lane(label: &str) -> String {
    if let Some(n) = label.strip_prefix("pan ") {
        return tr_fmt("pan {n}", &[("n", n)]);
    }
    if let Some(rest) = label.strip_prefix("send ") {
        for (suffix, key) in [(" level", "send {slot} level"), (" pan", "send {slot} pan"), (" mute", "send {slot} mute")] {
            if let Some(slot) = rest.strip_suffix(suffix) {
                return tr_fmt(key, &[("slot", slot)]);
            }
        }
    }
    if let Some(rest) = label.strip_prefix("insert ")
        && let Some((slot, param)) = rest.split_once(' ')
    {
        return tr_fmt("insert {slot} {param}", &[("slot", slot), ("param", &tr(param))]);
    }
    tr(label)
}

/// ダイアログ見出し。`メニュー — 項目` は、両側を別々に置き換える。
pub fn tr_title(title: &str) -> String {
    if lookup(title).is_some() {
        return tr(title);
    }
    if let Some((menu, item)) = title.split_once(" — ") {
        return format!("{} — {}", tr(menu), tr(item));
    }
    tr(title)
}

fn lookup(english: &str) -> Option<&'static str> {
    let rows = l10n_ja::JA;
    match rows.binary_search_by_key(&english, |row| row.0) {
        Ok(i) => rows.get(i).map(|row| row.1),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_sorted_and_nonempty() {
        let rows = l10n_ja::JA;
        assert!(rows.len() > 100);
        for pair in rows.windows(2) {
            let (Some(a), Some(b)) = (pair.first(), pair.get(1)) else { continue };
            assert!(a.0 < b.0, "unsorted or duplicate: {} / {}", a.0, b.0);
            assert!(!a.1.is_empty() && !b.1.is_empty());
        }
    }

    #[test]
    fn every_menu_label_is_in_the_catalog() {
        for line in soundcraft_engine::catalog::MENUS.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            for part in line.split(" > ") {
                assert!(lookup(part).is_some(), "untranslated menu label: {part}");
            }
        }
    }

    #[test]
    fn file_menu_is_japanese() {
        assert_eq!(tr("File"), "ファイル");
        assert_eq!(tr("not a label"), "not a label");
        assert_eq!(tr_fmt("Undo {action}", &[("action", "ペースト")]), "ペーストを元に戻す");
        assert_eq!(tr_grid("1/16 note dotted"), "16分音符 付点");
        assert_eq!(tr_lane("send A level"), "センド A レベル");
        assert_eq!(tr_lane("volume"), "音量");
    }
}
