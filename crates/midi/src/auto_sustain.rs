//! 選択ノートの和音変化に合わせてサスティン（CC64）を入れる。
//!
//! 同じ音高集合が続くあいだは踏み続け、和音が変わる位置で一度離して踏み直す。
//! 変わり目の隙間は短くし、前の和音が次へ濁らないようにする。
//! 選択範囲の外にある CC は触らない。

use crate::{CtrlEvent, CtrlKind, Note, PPQ};
use std::collections::BTreeSet;

/// サスティンペダルのコントローラ番号。
pub const SUSTAIN_CC: u8 = 64;

/// [`auto_sustain`] のパラメータ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoSustainOptions {
    /// 和音変わり目でペダル OFF→ON のあいだのティック数。
    /// 既定は 32 分音符相当。打ち込みのレガート・ペダリングでよく使う短い隙間。
    pub gap_ticks: i64,
    pub pedal_on: u8,
    pub pedal_off: u8,
}

impl Default for AutoSustainOptions {
    fn default() -> Self {
        Self { gap_ticks: i64::from(PPQ) / 32, pedal_on: 127, pedal_off: 0 }
    }
}

fn is_sustain(c: &CtrlEvent) -> bool {
    matches!(c.kind, CtrlKind::Cc(SUSTAIN_CC))
}

fn pitch_set(notes: &[&Note]) -> BTreeSet<u8> {
    notes.iter().map(|n| n.pitch).collect()
}

/// `notes` は選択範囲のノート。`ctrls` のうち、その時間範囲内・該当チャンネルの
/// 既存 CC64 だけを差し替え、範囲外の CC は残す。
///
/// 戻り値は書き込んだペダルイベント数。
pub fn auto_sustain(notes: &[Note], ctrls: &mut Vec<CtrlEvent>, opts: &AutoSustainOptions) -> usize {
    if notes.is_empty() {
        return 0;
    }
    let gap = opts.gap_ticks.max(1);
    let mut channels: BTreeSet<u8> = BTreeSet::new();
    for n in notes {
        channels.insert(n.channel);
    }
    let sel_start = notes.iter().map(|n| n.start).min().unwrap_or(0);
    let sel_end = notes.iter().map(|n| n.end()).max().unwrap_or(sel_start);

    // 範囲内・対象チャンネルの既存サスティンだけ外す。
    ctrls.retain(|c| !(is_sustain(c) && channels.contains(&c.channel) && c.tick >= sel_start && c.tick <= sel_end));

    let mut written = 0usize;
    for &ch in &channels {
        let mut ch_notes: Vec<&Note> = notes.iter().filter(|n| n.channel == ch).collect();
        if ch_notes.is_empty() {
            continue;
        }
        ch_notes.sort_by_key(|n| (n.start, n.pitch));

        // 同時発音を和音グループにする。
        let mut groups: Vec<(i64, i64, BTreeSet<u8>)> = Vec::new();
        let mut i = 0;
        while i < ch_notes.len() {
            let t = ch_notes[i].start;
            let mut j = i;
            while j < ch_notes.len() && ch_notes[j].start == t {
                j += 1;
            }
            let slice = &ch_notes[i..j];
            let end = slice.iter().map(|n| n.end()).max().unwrap_or(t);
            groups.push((t, end, pitch_set(slice)));
            i = j;
        }
        if groups.is_empty() {
            continue;
        }

        // 連続して同じ音高集合なら一つのペダル区間にまとめる。
        let mut regions: Vec<(i64, i64, BTreeSet<u8>)> = Vec::new();
        for (start, end, pitches) in groups {
            if let Some(last) = regions.last_mut()
                && last.2 == pitches
            {
                last.1 = last.1.max(end);
            } else {
                regions.push((start, end, pitches));
            }
        }

        let mut events: Vec<CtrlEvent> = Vec::new();
        let (first_start, _, _) = regions[0];
        events.push(CtrlEvent { tick: first_start, channel: ch, kind: CtrlKind::Cc(SUSTAIN_CC), value: i32::from(opts.pedal_on) });

        for w in regions.windows(2) {
            let (Some(&(_, _, _)), Some(&(next_start, _, _))) = (w.first(), w.get(1)) else {
                continue;
            };
            // 次の和音の直前で離し、短い隙間のあとで踏み直す。
            let off_at = next_start.saturating_sub(gap);
            let on_at = next_start;
            if off_at > first_start {
                events.push(CtrlEvent { tick: off_at, channel: ch, kind: CtrlKind::Cc(SUSTAIN_CC), value: i32::from(opts.pedal_off) });
            }
            events.push(CtrlEvent { tick: on_at, channel: ch, kind: CtrlKind::Cc(SUSTAIN_CC), value: i32::from(opts.pedal_on) });
        }

        let last_end = regions.last().map(|r| r.1).unwrap_or(first_start);
        events.push(CtrlEvent { tick: last_end, channel: ch, kind: CtrlKind::Cc(SUSTAIN_CC), value: i32::from(opts.pedal_off) });

        // 同じティックに OFF と ON が重ならないよう、tick 昇順で安定ソート。
        events.sort_by_key(|e| (e.tick, e.value));
        written += events.len();
        ctrls.extend(events);
    }

    ctrls.sort_by_key(|c| c.tick);
    let _ = (sel_start, sel_end);
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(pitch: u8, start: i64, length: i64) -> Note {
        Note { pitch, velocity: 100, release_velocity: 64, channel: 0, start, length }
    }

    #[test]
    fn re_pedals_when_chord_changes() {
        let notes = vec![n(60, 0, 480), n(64, 0, 480), n(67, 0, 480), n(65, 960, 480), n(69, 960, 480), n(72, 960, 480)];
        let mut ctrls = Vec::new();
        let opts = AutoSustainOptions::default();
        let written = auto_sustain(&notes, &mut ctrls, &opts);
        assert!(written >= 3);
        let sustains: Vec<_> = ctrls.iter().filter(|c| is_sustain(c)).collect();
        // 最初 ON、変わり目 OFF→ON、最後 OFF。
        assert!(sustains.iter().any(|c| c.tick == 0 && c.value == 127));
        assert!(sustains.iter().any(|c| c.tick == 960 - opts.gap_ticks && c.value == 0));
        assert!(sustains.iter().any(|c| c.tick == 960 && c.value == 127));
        assert!(sustains.iter().any(|c| c.tick == 960 + 480 && c.value == 0));
    }

    #[test]
    fn keeps_pedal_across_same_chord() {
        let notes = vec![n(60, 0, 200), n(64, 0, 200), n(60, 480, 200), n(64, 480, 200)];
        let mut ctrls = Vec::new();
        auto_sustain(&notes, &mut ctrls, &AutoSustainOptions::default());
        let offs: Vec<_> = ctrls.iter().filter(|c| is_sustain(c) && c.value == 0).collect();
        // 同じ和音の繰り返しでは途中で離さない。終わりの OFF だけ。
        assert_eq!(offs.len(), 1);
        assert_eq!(offs[0].tick, 680);
    }

    #[test]
    fn leaves_cc_outside_selection_alone() {
        let notes = vec![n(60, 1000, 100), n(64, 1000, 100)];
        let mut ctrls = vec![
            CtrlEvent { tick: 0, channel: 0, kind: CtrlKind::Cc(SUSTAIN_CC), value: 127 },
            CtrlEvent { tick: 50, channel: 0, kind: CtrlKind::Cc(SUSTAIN_CC), value: 0 },
            CtrlEvent { tick: 5000, channel: 0, kind: CtrlKind::Cc(7), value: 90 },
        ];
        auto_sustain(&notes, &mut ctrls, &AutoSustainOptions::default());
        assert!(ctrls.iter().any(|c| c.tick == 0 && is_sustain(c) && c.value == 127));
        assert!(ctrls.iter().any(|c| c.tick == 50 && is_sustain(c) && c.value == 0));
        assert!(ctrls.iter().any(|c| c.tick == 5000 && matches!(c.kind, CtrlKind::Cc(7))));
    }
}
