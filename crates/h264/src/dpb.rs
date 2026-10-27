//! Decoded picture buffer: reference marking (8.2.5), reference list construction (8.2.4) and output
//! ordering / bumping (C.4).

use crate::error::{Result, ensure};
use crate::picture::{FrameRef, RefPic};
use crate::slice::{Mmco, RefPicListMod, SliceHeader, SliceType};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefMark {
    Unused,
    Short,
    Long,
}

/// Output-related metadata attached to a decoded frame.
#[derive(Clone, Debug)]
pub struct OutputMeta {
    pub pts: i64,
    pub key: bool,
    pub crop: (u32, u32, u32, u32),
    pub full_range: bool,
    pub colour_primaries: u8,
    pub transfer_characteristics: u8,
    pub matrix_coefficients: u8,
    pub sar: (u16, u16),
    /// Decoded without deblocking (draft mode, non-reference picture).
    pub draft: bool,
}

pub struct DpbEntry {
    pub frame: FrameRef,
    pub poc: i32,
    pub frame_num: u32,
    pub long_term_frame_idx: u32,
    pub mark: RefMark,
    pub needed_for_output: bool,
    pub non_existing: bool,
    pub meta: Arc<OutputMeta>,
}

pub struct Dpb {
    pub entries: Vec<DpbEntry>,
    /// None = "no long-term frame indices".
    pub max_long_term_frame_idx: Option<u32>,
    pub capacity: usize,
    pub max_reorder: usize,
}

/// A picture leaving the DPB for output.
pub struct Output {
    pub frame: FrameRef,
    pub poc: i32,
    pub meta: Arc<OutputMeta>,
}

impl Dpb {
    pub fn new() -> Self {
        Dpb { entries: Vec::new(), max_long_term_frame_idx: None, capacity: 16, max_reorder: 16 }
    }

    fn frame_num_wrap(e: &DpbEntry, cur_frame_num: u32, max_frame_num: u32) -> i32 {
        if e.frame_num > cur_frame_num { e.frame_num as i32 - max_frame_num as i32 } else { e.frame_num as i32 }
    }

    fn num_refs(&self) -> usize {
        self.entries.iter().filter(|e| e.mark != RefMark::Unused).count()
    }

    /// Remove entries not needed for output and not used for reference.
    fn prune(&mut self) {
        self.entries.retain(|e| e.needed_for_output || e.mark != RefMark::Unused);
    }

    /// Output the picture with the smallest POC (C.4.5.3 "bumping"). Returns false if none waits.
    fn bump(&mut self, out: &mut Vec<Output>) -> bool {
        let Some(i) = self.entries.iter().enumerate().filter(|(_, e)| e.needed_for_output).min_by_key(|(_, e)| e.poc).map(|(i, _)| i) else {
            return false;
        };
        let e = &mut self.entries[i];
        e.needed_for_output = false;
        out.push(Output { frame: e.frame.clone(), poc: e.poc, meta: e.meta.clone() });
        self.prune();
        true
    }

    /// Output everything in POC order and clear non-reference entries.
    pub fn flush(&mut self, out: &mut Vec<Output>) {
        while self.bump(out) {}
        self.prune();
    }

    /// IDR: mark all references unused; output (or drop) prior pictures.
    pub fn idr(&mut self, no_output_of_prior_pics: bool, out: &mut Vec<Output>) {
        for e in &mut self.entries {
            e.mark = RefMark::Unused;
        }
        if no_output_of_prior_pics {
            self.entries.clear();
        } else {
            self.flush(out);
        }
        self.max_long_term_frame_idx = None;
    }

    /// Sliding window marking (8.2.5.3).
    fn sliding_window(&mut self, cur_frame_num: u32, max_frame_num: u32, max_num_ref_frames: usize) {
        let limit = max_num_ref_frames.max(1);
        while self.num_refs() >= limit {
            let victim = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.mark == RefMark::Short)
                .min_by_key(|(_, e)| Self::frame_num_wrap(e, cur_frame_num, max_frame_num))
                .map(|(i, _)| i);
            match victim {
                Some(i) => self.entries[i].mark = RefMark::Unused,
                None => break,
            }
        }
        self.prune();
    }

    /// Insert "non-existing" frames for a frame_num gap (8.2.5.2).
    #[allow(clippy::too_many_arguments)]
    pub fn fill_frame_num_gap(
        &mut self,
        prev_ref_frame_num: u32,
        frame_num: u32,
        max_frame_num: u32,
        max_num_ref_frames: usize,
        make_frame: &mut dyn FnMut(u32) -> (FrameRef, Arc<OutputMeta>),
        poc_state: &mut dyn FnMut(u32),
    ) {
        if max_frame_num == 0 {
            return;
        }
        let mut unused = (prev_ref_frame_num + 1) % max_frame_num;
        // Only the last `max_num_ref_frames` "non-existing" frames can survive the sliding window, and
        // inserting more than that evicts every earlier short-term reference. Skip straight to that tail
        // (keeping the POC state walk for every missing frame_num) so a hostile gap of up to 65535
        // frames costs at most `max_num_ref_frames` frame allocations instead of one per missing frame.
        let gap = frame_num.wrapping_add(max_frame_num).wrapping_sub(unused) % max_frame_num;
        let keep = u32::try_from(max_num_ref_frames.max(1)).unwrap_or(u32::MAX);
        let skip = gap.saturating_sub(keep);
        if skip > 0 {
            for e in self.entries.iter_mut().filter(|e| e.mark == RefMark::Short) {
                e.mark = RefMark::Unused;
            }
            self.prune();
            for _ in 0..skip {
                poc_state(unused);
                unused = (unused + 1) % max_frame_num;
            }
        }
        let mut guard = 0;
        while unused != frame_num && guard < max_frame_num {
            self.sliding_window(unused, max_frame_num, max_num_ref_frames);
            let (frame, meta) = make_frame(unused);
            poc_state(unused);
            self.entries.push(DpbEntry {
                poc: frame.poc,
                frame,
                frame_num: unused,
                long_term_frame_idx: 0,
                mark: RefMark::Short,
                needed_for_output: false,
                non_existing: true,
                meta,
            });
            unused = (unused + 1) % max_frame_num;
            guard += 1;
        }
    }

    /// Initial reference picture lists (8.2.4.2) as DPB entries.
    fn initial_entry_lists<'a>(
        sh: &SliceHeader,
        cur_poc: i32,
        short: &[&'a DpbEntry],
        long: &[&'a DpbEntry],
        pic_num: impl Fn(&DpbEntry) -> i32,
    ) -> [Vec<&'a DpbEntry>; 2] {
        let mut entry_lists: [Vec<&'a DpbEntry>; 2] = [Vec::new(), Vec::new()];
        if sh.slice_type != SliceType::B {
            let mut s = short.to_vec();
            s.sort_by_key(|e| std::cmp::Reverse(pic_num(e)));
            entry_lists[0] = s.into_iter().chain(long.iter().copied()).collect();
        } else {
            let mut before: Vec<&'a DpbEntry> = short.iter().copied().filter(|e| e.poc < cur_poc).collect();
            let mut after: Vec<&'a DpbEntry> = short.iter().copied().filter(|e| e.poc > cur_poc).collect();
            before.sort_by_key(|e| std::cmp::Reverse(e.poc));
            after.sort_by_key(|e| e.poc);
            entry_lists[0] = before.iter().chain(after.iter()).chain(long.iter()).copied().collect();
            entry_lists[1] = after.iter().chain(before.iter()).chain(long.iter()).copied().collect();
            if entry_lists[1].len() > 1 && entry_lists[0].len() == entry_lists[1].len() {
                let same = entry_lists[0].iter().zip(entry_lists[1].iter()).all(|(a, b)| std::ptr::eq(*a, *b));
                if same {
                    entry_lists[1].swap(0, 1);
                }
            }
        }
        entry_lists
    }

    /// picNumLXNoWrap (8-34, 8-35) of a short-term modification from the predictor `pred`, and the
    /// picNum it selects.
    fn short_term_pic_nums(pred: i32, idc: u32, value: u32, curr_pic_num: i32, max_pic_num: i32) -> (i32, i32) {
        let d = value as i32 + 1;
        let no_wrap = if idc == 0 {
            if pred - d < 0 { pred - d + max_pic_num } else { pred - d }
        } else if pred + d >= max_pic_num {
            pred + d - max_pic_num
        } else {
            pred + d
        };
        let pn = if no_wrap > curr_pic_num { no_wrap - max_pic_num } else { no_wrap };
        (no_wrap, pn)
    }

    /// Puts `target` at `ref_idx` and shifts the rest of the list up, then drops the later entries
    /// that repeat it (8.2.4.3.1, 8.2.4.3.2) and keeps the list `n` long.
    fn insert_ref<'a>(list: &mut Vec<Option<&'a DpbEntry>>, ref_idx: usize, n: usize, target: &'a DpbEntry, is_long: bool) {
        for c in (ref_idx + 1..=n).rev() {
            list[c] = list[c - 1];
        }
        list[ref_idx] = Some(target);
        let mut nidx = ref_idx + 1;
        for c in ref_idx + 1..=n {
            let keep = match list[c] {
                None => true,
                Some(e) => {
                    if is_long {
                        !(e.mark == RefMark::Long && e.long_term_frame_idx == target.long_term_frame_idx)
                    } else {
                        !(e.mark == RefMark::Short && std::ptr::eq(e, target))
                    }
                }
            };
            if keep {
                list[nidx] = list[c];
                nidx += 1;
            }
        }
        list.truncate(n);
    }

    /// Reference picture list modification (8.2.4.3) of one list; `n` is num_ref_idx_active.
    fn modify_ref_list<'a>(
        list: &mut Vec<Option<&'a DpbEntry>>,
        mods: &[RefPicListMod],
        n: usize,
        cur_fn: u32,
        max_frame_num: u32,
        short: &[&'a DpbEntry],
        long: &[&'a DpbEntry],
    ) -> Result<()> {
        let max_pic_num = max_frame_num as i32;
        let curr_pic_num = cur_fn as i32;
        let mut pred = curr_pic_num;
        let mut ref_idx = 0usize;
        for m in mods {
            ensure!(ref_idx < n, "too many ref_pic_list_modification entries");
            list.push(None); // temporarily one longer
            let (target, is_long): (Option<&DpbEntry>, bool) = match m.idc {
                0 | 1 => {
                    let (no_wrap, pn) = Self::short_term_pic_nums(pred, m.idc, m.value, curr_pic_num, max_pic_num);
                    pred = no_wrap;
                    (short.iter().copied().find(|e| Self::frame_num_wrap(e, cur_fn, max_frame_num) == pn), false)
                }
                2 => (long.iter().copied().find(|e| e.long_term_frame_idx == m.value), true),
                _ => continue,
            };
            let Some(target) = target else {
                // Missing picture: keep the list unchanged (error concealment).
                list.pop();
                ref_idx += 1;
                continue;
            };
            Self::insert_ref(list, ref_idx, n, target, is_long);
            ref_idx += 1;
        }
        Ok(())
    }

    /// Initial reference picture lists (8.2.4.2) + modification (8.2.4.3) for a frame slice.
    pub fn build_ref_lists(&self, sh: &SliceHeader, cur_poc: i32, max_frame_num: u32) -> Result<[Vec<RefPic>; 2]> {
        let mut lists: [Vec<RefPic>; 2] = [Vec::new(), Vec::new()];
        if sh.slice_type.is_intra() {
            return Ok(lists);
        }
        let cur_fn = sh.frame_num;
        let pic_num = |e: &DpbEntry| Self::frame_num_wrap(e, cur_fn, max_frame_num);
        let short: Vec<&DpbEntry> = self.entries.iter().filter(|e| e.mark == RefMark::Short).collect();
        let mut long: Vec<&DpbEntry> = self.entries.iter().filter(|e| e.mark == RefMark::Long).collect();
        long.sort_by_key(|e| e.long_term_frame_idx);
        let to_ref = |e: &&DpbEntry| RefPic { frame: e.frame.clone(), long_term: e.mark == RefMark::Long };
        let entry_lists = Self::initial_entry_lists(sh, cur_poc, &short, &long, pic_num);
        let nlists = if sh.slice_type == SliceType::B { 2 } else { 1 };
        for l in 0..nlists {
            let n = sh.num_ref_idx_active[l] as usize;
            let mut list: Vec<Option<&DpbEntry>> = entry_lists[l].iter().copied().map(Some).collect();
            list.truncate(n);
            list.resize(n, None);
            if !sh.ref_pic_list_mod[l].is_empty() {
                Self::modify_ref_list(&mut list, &sh.ref_pic_list_mod[l], n, cur_fn, max_frame_num, &short, &long)?;
            }
            // Fill missing entries (should not happen in conforming streams) with the first valid entry.
            let fallback = list.iter().flatten().next().copied().or_else(|| self.entries.last());
            let out: Vec<RefPic> = list.iter().filter_map(|e| e.or(fallback)).map(|e| to_ref(&e)).collect();
            lists[l] = out;
        }
        Ok(lists)
    }

    /// Marks as unused every entry in state `mark` that `hit` selects.
    fn unmark_where(&mut self, mark: RefMark, hit: impl Fn(&DpbEntry) -> bool) {
        for e in &mut self.entries {
            if e.mark == mark && hit(e) {
                e.mark = RefMark::Unused;
            }
        }
    }

    /// Makes every short-term entry that `hit` selects a long-term one with index `idx`.
    fn mark_long_where(&mut self, hit: impl Fn(&DpbEntry) -> bool, idx: u32) {
        for e in &mut self.entries {
            if e.mark == RefMark::Short && hit(e) {
                e.mark = RefMark::Long;
                e.long_term_frame_idx = idx;
            }
        }
    }

    /// One memory management control operation; returns true for MMCO 5.
    fn apply_mmco(&mut self, m: &Mmco, cur_frame_num: u32, max_frame_num: u32, cur_long: &mut Option<u32>) -> bool {
        let mut had5 = false;
        let curr_pic_num = cur_frame_num as i32;
        match m.op {
            1 => {
                let pn = curr_pic_num - (m.difference_of_pic_nums_minus1 as i32 + 1);
                self.unmark_where(RefMark::Short, |e| Self::frame_num_wrap(e, cur_frame_num, max_frame_num) == pn);
            }
            2 => self.unmark_where(RefMark::Long, |e| e.long_term_frame_idx == m.long_term_pic_num),
            3 => {
                let pn = curr_pic_num - (m.difference_of_pic_nums_minus1 as i32 + 1);
                self.unmark_where(RefMark::Long, |e| e.long_term_frame_idx == m.long_term_frame_idx);
                self.mark_long_where(|e| Self::frame_num_wrap(e, cur_frame_num, max_frame_num) == pn, m.long_term_frame_idx);
            }
            4 => {
                let max = m.max_long_term_frame_idx_plus1;
                self.unmark_where(RefMark::Long, |e| max == 0 || e.long_term_frame_idx > max - 1);
                self.max_long_term_frame_idx = if max == 0 { None } else { Some(max - 1) };
            }
            5 => {
                for e in &mut self.entries {
                    e.mark = RefMark::Unused;
                }
                self.max_long_term_frame_idx = None;
                had5 = true;
            }
            6 => {
                // Only an index within the allowed range may be assigned (8.2.5.4.6).
                let allowed = self.max_long_term_frame_idx.is_some_and(|max| m.long_term_frame_idx <= max);
                if allowed {
                    self.unmark_where(RefMark::Long, |e| e.long_term_frame_idx == m.long_term_frame_idx);
                    *cur_long = Some(m.long_term_frame_idx);
                }
            }
            _ => {}
        }
        had5
    }

    /// Adaptive memory control (8.2.5.4). Returns true if MMCO 5 was executed.
    fn apply_mmcos(&mut self, mmcos: &[Mmco], cur_frame_num: u32, max_frame_num: u32, cur_long: &mut Option<u32>) -> bool {
        let mut had5 = false;
        for m in mmcos {
            had5 |= self.apply_mmco(m, cur_frame_num, max_frame_num, cur_long);
        }
        self.prune();
        had5
    }

    /// Reference marking of the current picture (8.2.5); returns (mark, long-term idx, MMCO 5 executed).
    fn mark_current(&mut self, sh: &SliceHeader, max_frame_num: u32, max_num_ref_frames: usize) -> (RefMark, u32, bool) {
        let mut mark = RefMark::Unused;
        let mut long_idx = 0;
        let mut had5 = false;
        if sh.nal_ref_idc != 0 {
            if sh.idr {
                if sh.long_term_reference {
                    mark = RefMark::Long;
                    self.max_long_term_frame_idx = Some(0);
                } else {
                    mark = RefMark::Short;
                    self.max_long_term_frame_idx = None;
                }
            } else {
                let mut cur_long = None;
                if sh.adaptive_ref_pic_marking {
                    had5 = self.apply_mmcos(&sh.mmcos, sh.frame_num, max_frame_num, &mut cur_long);
                } else {
                    self.sliding_window(sh.frame_num, max_frame_num, max_num_ref_frames);
                }
                if let Some(i) = cur_long {
                    mark = RefMark::Long;
                    long_idx = i;
                } else {
                    mark = RefMark::Short;
                    // make room if MMCOs left the DPB over-full (non-conforming streams)
                    if self.num_refs() >= max_num_ref_frames.max(1) && sh.adaptive_ref_pic_marking {
                        self.sliding_window(sh.frame_num, max_frame_num, max_num_ref_frames);
                    }
                }
            }
        }
        (mark, long_idx, had5)
    }

    /// Bumping until there is space (C.4.5.1 / C.4.5.2). Returns true when the (non-reference) current
    /// picture has the smallest POC and is to be output directly instead of being stored.
    fn bump_for_space(&mut self, mark: RefMark, poc: i32, out: &mut Vec<Output>) -> bool {
        if mark == RefMark::Unused {
            loop {
                let waiting = self.entries.len();
                if waiting < self.capacity {
                    break;
                }
                // non-reference picture with the smallest POC is output directly
                let min_poc = self.entries.iter().filter(|e| e.needed_for_output).map(|e| e.poc).min();
                if min_poc.is_none_or(|m| poc < m) {
                    return true;
                }
                if !self.bump(out) {
                    break;
                }
            }
        } else {
            while self.entries.len() >= self.capacity {
                if !self.bump(out) {
                    if let Some(pos) = self.entries.iter().position(|e| !e.needed_for_output) {
                        self.entries.remove(pos);
                    } else {
                        self.entries.remove(0);
                    }
                    break;
                }
            }
        }
        false
    }

    /// Reference marking for the just-decoded picture and insertion into the DPB with output bumping.
    /// Returns pictures ready for output.
    #[allow(clippy::too_many_arguments)]
    pub fn store_picture(
        &mut self,
        sh: &SliceHeader,
        frame: FrameRef,
        poc: i32,
        max_frame_num: u32,
        max_num_ref_frames: usize,
        meta: Arc<OutputMeta>,
        out: &mut Vec<Output>,
    ) {
        let (mark, long_idx, had5) = self.mark_current(sh, max_frame_num, max_num_ref_frames);
        if had5 {
            // All earlier pictures are output before the current one (C.4.4).
            self.flush(out);
        }
        let frame_num = if had5 { 0 } else { sh.frame_num };
        if self.bump_for_space(mark, poc, out) {
            out.push(Output { frame, poc, meta });
            return;
        }
        self.entries.push(DpbEntry {
            frame,
            poc,
            frame_num,
            long_term_frame_idx: long_idx,
            mark,
            needed_for_output: true,
            non_existing: false,
            meta,
        });
        // Low-delay output: honour max_num_reorder_frames.
        while self.entries.iter().filter(|e| e.needed_for_output).count() > self.max_reorder {
            if !self.bump(out) {
                break;
            }
        }
    }
}

impl Default for Dpb {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picture::Frame;

    fn meta() -> Arc<OutputMeta> {
        Arc::new(OutputMeta {
            pts: 0,
            key: false,
            crop: (0, 0, 0, 0),
            full_range: false,
            colour_primaries: 2,
            transfer_characteristics: 2,
            matrix_coefficients: 2,
            sar: (1, 1),
            draft: false,
        })
    }

    fn short(dpb: &mut Dpb, id: u32, frame_num: u32) {
        dpb.entries.push(DpbEntry {
            frame: Arc::new(Frame::new(id, 0, 1, 1)),
            poc: 0,
            frame_num,
            long_term_frame_idx: 0,
            mark: RefMark::Short,
            needed_for_output: false,
            non_existing: false,
            meta: meta(),
        });
    }

    /// Fill a gap and return (frames made, POC walk, reference frame_nums left in the DPB).
    fn fill(dpb: &mut Dpb, prev: u32, cur: u32, max: u32, refs: usize) -> (u32, Vec<u32>, Vec<(u32, bool)>) {
        let mut made = 0u32;
        let mut walk = Vec::new();
        let mut make = |_f: u32| {
            made += 1;
            (Arc::new(Frame::new(1000 + made, 0, 1, 1)), meta())
        };
        let mut poc = |f: u32| walk.push(f);
        dpb.fill_frame_num_gap(prev, cur, max, refs, &mut make, &mut poc);
        let left = dpb.entries.iter().filter(|e| e.mark != RefMark::Unused).map(|e| (e.frame_num, e.non_existing)).collect();
        (made, walk, left)
    }

    #[test]
    fn huge_gap_makes_only_the_frames_that_stay_referenced() {
        let mut dpb = Dpb::new();
        short(&mut dpb, 1, 0);
        short(&mut dpb, 2, 1);
        let (made, walk, left) = fill(&mut dpb, 1, 65535, 65536, 4);
        assert_eq!(made, 4);
        // The POC state still walks every missing frame_num, in order.
        assert_eq!(walk.len(), 65533);
        assert_eq!(walk.first(), Some(&2));
        assert_eq!(walk.last(), Some(&65534));
        assert_eq!(left, vec![(65531, true), (65532, true), (65533, true), (65534, true)]);
    }

    #[test]
    fn huge_gap_across_wraparound_matches_the_frame_by_frame_result() {
        let mut dpb = Dpb::new();
        short(&mut dpb, 1, 65000);
        let (made, walk, left) = fill(&mut dpb, 65000, 3, 65536, 2);
        assert_eq!(made, 2);
        assert_eq!(walk.len(), 538);
        assert_eq!(walk.first(), Some(&65001));
        assert_eq!(walk.last(), Some(&2));
        assert_eq!(left, vec![(1, true), (2, true)]);
    }

    #[test]
    fn small_gap_keeps_earlier_references() {
        let mut dpb = Dpb::new();
        short(&mut dpb, 1, 0);
        short(&mut dpb, 2, 1);
        let (made, walk, left) = fill(&mut dpb, 1, 4, 16, 4);
        assert_eq!(made, 2);
        assert_eq!(walk, vec![2, 3]);
        assert_eq!(left, vec![(0, false), (1, false), (2, true), (3, true)]);
    }

    #[test]
    fn long_term_references_survive_a_huge_gap() {
        let mut dpb = Dpb::new();
        short(&mut dpb, 1, 0);
        if let Some(e) = dpb.entries.last_mut() {
            e.mark = RefMark::Long;
        }
        let (made, _, left) = fill(&mut dpb, 0, 1000, 1024, 3);
        assert!(made <= 3);
        assert_eq!(left, vec![(0, false), (998, true), (999, true)]);
    }
}
