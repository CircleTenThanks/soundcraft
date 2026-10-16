//! 選択ノートへ、拍・和音・音高の流れ・フレーズからベロシティを付ける。
//!
//! 乱数は使わない。数値の目安は、J-pop／ポップス向けキーボード打ち込みの定石
//! （強拍＞中強拍＞弱拍、オモテ＞ウラでおおよそ −20、外声を内声より強め、
//! フレーズは入りを抑え山を高く終わりを落とす）に合わせる。

use crate::{Note, PPQ};

/// ノート開始位置の拍情報。テンポマップ側で埋める。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteBeat {
    /// 小節内の拍（1始まり）。
    pub beat_in_bar: u32,
    pub beats_per_bar: u32,
    /// 拍内ティック。
    pub tick_in_beat: i64,
    pub ticks_per_beat: i64,
}

/// [`shape_velocity`] のパラメータ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeVelocityOptions {
    /// 基準ベロシティ。ベタ打ちの目安に合わせ既定は 100。
    pub strength: u8,
    /// 強弱の差 0..=100。100 で文献どおりの拍差（ウラ −20 など）をそのまま使う。
    pub contrast: u8,
    /// フレーズ区切りとみなす休符の長さ（ティック）。既定は 2 分音符相当。
    pub phrase_gap_ticks: i64,
}

impl Default for ShapeVelocityOptions {
    fn default() -> Self {
        Self {
            strength: 100,
            contrast: 100,
            // ポップス伴奏の息継ぎに合わせ、2分音符以上の空きでフレーズを分ける。
            phrase_gap_ticks: i64::from(PPQ) * 2,
        }
    }
}

fn clamp_vel(v: f64) -> u8 {
    v.round().clamp(1.0, 127.0) as u8
}

fn unit_contrast(c: u8) -> f64 {
    f64::from(c).clamp(0.0, 100.0) / 100.0
}

/// 4/4 なら 1 が最も強く、次に 3、2 と 4 は弱く。その他の拍子も強拍／中強拍／弱拍に同じ関係を当てる。
fn beat_delta(beat: u32, beats_per_bar: u32) -> f64 {
    if beats_per_bar == 0 || beat == 0 {
        return 0.0;
    }
    if beat == 1 {
        return 0.0;
    }
    if beats_per_bar == 3 {
        // 3/4: 1 が強拍、2 と 3 はその次。
        return -12.0;
    }
    let mid = if beats_per_bar.is_multiple_of(2) { beats_per_bar / 2 + 1 } else { 0 };
    if mid > 0 && beat == mid {
        return -10.0;
    }
    -20.0
}

fn on_beat_head(b: &NoteBeat) -> bool {
    let tpb = b.ticks_per_beat.max(1);
    // 拍頭付近（拍の 1/8 以内）を表拍とみなす。
    b.tick_in_beat <= tpb / 8
}

fn is_syncopation(note: &Note, b: &NoteBeat) -> bool {
    if on_beat_head(b) {
        return false;
    }
    let tpb = b.ticks_per_beat.max(1);
    // ウラから始まり、次の拍頭をまたぐ長さならシンコペとして強める。
    let to_next = tpb.saturating_sub(b.tick_in_beat);
    note.length >= to_next && b.tick_in_beat >= tpb / 4
}

/// `beats` は `notes` と同じ長さ。短い方の長さまでしか処理しない。
pub fn shape_velocity(notes: &mut [Note], beats: &[NoteBeat], opts: &ShapeVelocityOptions) {
    let n = notes.len().min(beats.len());
    if n == 0 {
        return;
    }
    let c = unit_contrast(opts.contrast);
    let base = f64::from(opts.strength);
    let gap = opts.phrase_gap_ticks.max(1);

    // 開始順のインデックス（フレーズ・流れ用）。
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| (notes[i].start, notes[i].channel, notes[i].pitch));

    // フレーズ ID（長い休符で区切る）。
    let mut phrase_of = vec![0usize; n];
    let mut phrase = 0usize;
    let mut prev_end = notes[order[0]].end();
    phrase_of[order[0]] = 0;
    for &i in order.iter().skip(1) {
        let start = notes[i].start;
        if start.saturating_sub(prev_end) >= gap {
            phrase = phrase.saturating_add(1);
        }
        phrase_of[i] = phrase;
        prev_end = prev_end.max(notes[i].end());
    }
    let phrase_count = phrase.saturating_add(1);
    let mut phrase_span: Vec<(i64, i64)> = vec![(i64::MAX, i64::MIN); phrase_count];
    for &i in &order {
        let p = phrase_of[i];
        let (lo, hi) = &mut phrase_span[p];
        *lo = (*lo).min(notes[i].start);
        *hi = (*hi).max(notes[i].start);
    }

    // 同時刻のメロディ線（各開始の最高音）で音高の流れを追う。
    let mut melody: Vec<(i64, u8, usize)> = Vec::new();
    {
        let mut i = 0;
        while i < order.len() {
            let t = notes[order[i]].start;
            let mut best = order[i];
            let mut j = i;
            while j < order.len() && notes[order[j]].start == t {
                if notes[order[j]].pitch >= notes[best].pitch {
                    best = order[j];
                }
                j += 1;
            }
            melody.push((t, notes[best].pitch, best));
            i = j;
        }
    }
    let mut contour = vec![0.0f64; n];
    for w in melody.windows(2) {
        let (Some(&(_, p0, _)), Some(&(_, p1, idx))) = (w.first(), w.get(1)) else { continue };
        let dp = i32::from(p1) - i32::from(p0);
        let leap = dp.unsigned_abs() >= 5;
        let delta = if leap {
            // 跳躍の着地は順次進行より強く。
            8.0
        } else if dp > 0 {
            3.0
        } else if dp < 0 {
            -3.0
        } else {
            0.0
        };
        contour[idx] += delta * c;
    }

    // 同時発音グループで外声／内声。
    let mut voice = vec![0.0f64; n];
    {
        let mut i = 0;
        while i < order.len() {
            let t = notes[order[i]].start;
            let mut group: Vec<usize> = Vec::new();
            let mut j = i;
            while j < order.len() && notes[order[j]].start == t {
                group.push(order[j]);
                j += 1;
            }
            if group.len() >= 2 {
                let mut lo = group[0];
                let mut hi = group[0];
                for &g in &group {
                    if notes[g].pitch < notes[lo].pitch {
                        lo = g;
                    }
                    if notes[g].pitch > notes[hi].pitch {
                        hi = g;
                    }
                }
                for &g in &group {
                    if g == lo || g == hi {
                        voice[g] += 10.0 * c;
                    } else {
                        voice[g] -= 5.0 * c;
                    }
                }
            }
            i = j;
        }
    }

    for i in 0..n {
        let b = &beats[i];
        let mut v = base;
        v += beat_delta(b.beat_in_bar, b.beats_per_bar) * c;
        if !on_beat_head(b) {
            if is_syncopation(&notes[i], b) {
                // シンコペは強拍相当まで戻す（ウラの −20 を打ち消す）。
            } else if b.tick_in_beat >= b.ticks_per_beat.max(1) / 2 {
                // 8分ウラ付近。
                v -= 20.0 * c;
            } else {
                // 16分ウラなど、さらに弱い位置。
                v -= 28.0 * c;
            }
        }
        v += voice[i];
        v += contour[i];

        let (plo, phi) = phrase_span[phrase_of[i]];
        let span = (phi - plo).max(1) as f64;
        let t = (notes[i].start - plo) as f64 / span;
        // 入りを抑え、山（おおよそ 2/3）を高く、終わりを落とす。
        let env = if t < 0.66 { 0.88 + (1.0 - 0.88) * (t / 0.66) } else { 1.0 - (1.0 - 0.90) * ((t - 0.66) / 0.34) };
        v = base + (v - base) * ((env - 1.0) * c + 1.0);
        // フレーズ包絡そのものも contrast に応じて掛ける。
        v *= 1.0 + (env - 1.0) * c * 0.5;

        notes[i].velocity = clamp_vel(v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(pitch: u8, start: i64, length: i64) -> Note {
        Note { pitch, velocity: 64, release_velocity: 64, channel: 0, start, length }
    }

    fn beat4(beat: u32, tick_in_beat: i64) -> NoteBeat {
        NoteBeat { beat_in_bar: beat, beats_per_bar: 4, tick_in_beat, ticks_per_beat: i64::from(PPQ) }
    }

    #[test]
    fn strong_beat_is_louder_than_weak_beat() {
        let mut notes = vec![note(60, 0, 100), note(60, 960, 100)];
        let beats = vec![beat4(1, 0), beat4(2, 0)];
        shape_velocity(&mut notes, &beats, &ShapeVelocityOptions::default());
        assert!(notes[0].velocity > notes[1].velocity);
    }

    #[test]
    fn outer_voices_exceed_inner() {
        let mut notes = vec![note(48, 0, 100), note(60, 0, 100), note(72, 0, 100)];
        let beats = vec![beat4(1, 0); 3];
        shape_velocity(&mut notes, &beats, &ShapeVelocityOptions::default());
        assert!(notes[0].velocity > notes[1].velocity);
        assert!(notes[2].velocity > notes[1].velocity);
    }

    #[test]
    fn ascending_step_raises_landing() {
        let mut notes = vec![note(60, 0, 100), note(62, 960, 100)];
        // どちらも拍頭。1拍目より2拍目は弱いが、上行の加点で着地が相対的に持ち上がる。
        let beats = vec![beat4(1, 0), beat4(2, 0)];
        let flat = ShapeVelocityOptions { contrast: 0, ..Default::default() };
        shape_velocity(&mut notes, &beats, &flat);
        assert_eq!(notes[0].velocity, notes[1].velocity);
        let mut notes = vec![note(60, 0, 100), note(62, 960, 100)];
        shape_velocity(&mut notes, &beats, &ShapeVelocityOptions::default());
        // 拍の弱さより上行の加点が効いていれば、着地は拍だけのときより強い。
        let mut only_beats = vec![note(60, 0, 100), note(60, 960, 100)];
        shape_velocity(&mut only_beats, &beats, &ShapeVelocityOptions::default());
        assert!(notes[1].velocity > only_beats[1].velocity);
    }

    #[test]
    fn zero_contrast_keeps_near_strength() {
        let mut notes = vec![note(60, 0, 100), note(64, 960, 100), note(67, 960, 100)];
        let beats = vec![beat4(1, 0), beat4(2, 0), beat4(2, 0)];
        shape_velocity(&mut notes, &beats, &ShapeVelocityOptions { contrast: 0, ..Default::default() });
        for n in &notes {
            assert_eq!(n.velocity, 100);
        }
    }
}
