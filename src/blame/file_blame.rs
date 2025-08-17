use std::{
    collections::BTreeMap,
    fmt::{Debug, Display},
    hash::Hash,
};

pub type LineNumber = u32;
pub type LineDelta = i64;

pub trait Keyable: Copy + PartialEq + Display + Debug + Eq + Hash {}
impl<T: Copy + PartialEq + Display + Debug + Eq + Hash> Keyable for T {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlameRange<CohortKey>
where
    CohortKey: Keyable,
{
    pub start_line: LineNumber,
    pub line_count: LineNumber,
    pub cohort: CohortKey,
}

impl<CohortKey: Keyable> BlameRange<CohortKey> {
    pub fn new(start_line: LineNumber, line_count: LineNumber, cohort: CohortKey) -> Self {
        Self {
            start_line,
            line_count,
            cohort,
        }
    }
}

/// FileBlame stores change-points: a mapping from starting line to cohort.
/// The end of each interval is implicit: the next key, or `total_lines` for the last one.
#[derive(Debug, Clone)]
pub struct FileBlame<CohortKey: Keyable> {
    change_points: BTreeMap<LineNumber, CohortKey>,
    total_lines: LineNumber,
}

impl<CohortKey: Keyable> FileBlame<CohortKey> {
    pub fn new(total_lines: LineNumber, cohort: CohortKey) -> Self {
        let mut change_points = BTreeMap::new();
        if total_lines > 0 {
            change_points.insert(0, cohort);
        }
        Self {
            change_points,
            total_lines,
        }
    }

    pub fn total_lines(&self) -> LineNumber {
        self.total_lines
    }

    pub fn range_count(&self) -> usize {
        self.change_points.len()
    }

    fn cohort_at_index(&self, index: LineNumber) -> Option<CohortKey> {
        if index >= self.total_lines {
            return None;
        }
        self.change_points
            .range(..=index)
            .next_back()
            .map(|(_, cohort)| *cohort)
    }

    pub fn blame_for_line(&self, line: LineNumber) -> Option<(LineNumber, LineNumber, CohortKey)> {
        if line >= self.total_lines {
            return None;
        }
        let (&start, &cohort) = self
            .change_points
            .range(..=line)
            .next_back()
            .expect("there must be a change-point before any valid line");
        let next_end = self
            .change_points
            .range((start + 1)..)
            .next()
            .map(|(&k, _)| k)
            .unwrap_or(self.total_lines);
        Some((start, next_end - start, cohort))
    }

    pub fn merge_adjacent_ranges(&mut self) {
        if self.change_points.is_empty() {
            return;
        }
        let mut prev_value: Option<CohortKey> = None;
        let mut keys_to_remove: Vec<LineNumber> = Vec::new();
        for (&k, &v) in self.change_points.iter() {
            if let Some(prev) = prev_value {
                if prev == v {
                    keys_to_remove.push(k);
                }
            }
            prev_value = Some(v);
        }
        for k in keys_to_remove {
            self.change_points.remove(&k);
        }
    }

    /// Get an iterator over ranges as (start_line, end_line, cohort)
    pub fn ranges(&self) -> impl Iterator<Item = (LineNumber, LineNumber, CohortKey)> + '_ {
        let mut iter = self.change_points.iter().peekable();
        std::iter::from_fn(move || {
            if let Some((&start, &cohort)) = iter.next() {
                let end = if let Some((next_start, _)) = iter.peek() {
                    **next_start
                } else {
                    self.total_lines
                };
                if end < start {
                    panic!(
                        "start is after end for range: {}, end: {}. next start: {:?}, total lines: {}",
                        start, end, iter.peek().map(|(k, _)| *k), self.total_lines
                    );
                }
                Some((start, end, cohort))
            } else {
                None
            }
        })
    }

    pub fn cohort_stats(&self) -> std::collections::HashMap<CohortKey, u64> {
        let mut stats = std::collections::HashMap::new();
        for (start, end, cohort) in self.ranges() {
            *stats.entry(cohort).or_insert(0) += (end - start) as u64;
        }
        stats
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.total_lines == 0 {
            if !self.change_points.is_empty() {
                return Err("Change-points must be empty when total_lines == 0".to_string());
            }
            return Ok(());
        }
        // First key must be 0
        match self.change_points.keys().next() {
            Some(&key) if key == 0 => {}
            _ => return Err("The first change-point must be at line 0".to_string()),
        }
        // Keys must be strictly increasing and strictly less than total_lines
        let mut prev_key: Option<LineNumber> = None;
        for &key in self.change_points.keys() {
            if key >= self.total_lines {
                return Err("Change-point key must be < total_lines".to_string());
            }
            if let Some(prev) = prev_key {
                if key <= prev {
                    return Err("Change-point keys must be strictly increasing".to_string());
                }
            }
            prev_key = Some(key);
        }
        Ok(())
    }

    // The main, and most important method in this file.
    // It applies a vector of line diffs to the file blame.
    // The diffs are given as a vector of tuples, where each tuple contains:
    // - A range of lines to delete
    // - A range of lines to insert
    // - The cohort to apply to the lines in the range after the diff
    // The diffs are applied in order, from bottom to top.
    //
    // We have property tests against a reference implementation to validate correctness.
    pub fn apply_line_diffs(
        &mut self,
        line_diffs: Vec<(
            std::ops::Range<LineNumber>,
            std::ops::Range<LineNumber>,
            CohortKey,
        )>,
    ) {
        if line_diffs.is_empty() {
            return;
        }
        let mut diffs = line_diffs.clone();
        diffs.sort_by_key(|(before, _, _)| before.start);

        let old_total = self.total_lines;
        let mut new_change_points: BTreeMap<LineNumber, CohortKey> = BTreeMap::new();
        let mut cp_iter = self.change_points.iter().peekable();
        let mut offset: LineDelta = 0;

        // Push change-point only if cohort changed from the last emitted
        let push_cp =
            |pos: LineNumber, cohort: CohortKey, map: &mut BTreeMap<LineNumber, CohortKey>| {
                if let Some((_, &last_cohort)) = map.last_key_value() {
                    if last_cohort == cohort {
                        return;
                    }
                }
                map.insert(pos, cohort);
            };

        for (before, after, cohort) in diffs.into_iter() {
            let before_start = before.start;
            let before_end = before.end;
            let before_len = before_end - before_start;
            let after_len = after.len() as LineNumber;
            let delta = after_len as LineDelta - before_len as LineDelta;

            // 1) Emit unaffected change-points before b0, shifted by current offset
            while let Some((&line, &line_cohort)) = cp_iter.peek().copied() {
                if line < before_start {
                    push_cp(
                        (line as LineDelta + offset) as LineNumber,
                        line_cohort,
                        &mut new_change_points,
                    );
                    cp_iter.next();
                } else {
                    break;
                }
            }

            // 2) Insert the new block's cohort at before_start if any insertion
            if after_len > 0 {
                push_cp(
                    (before_start as LineDelta + offset) as LineNumber,
                    cohort,
                    &mut new_change_points,
                );
            }

            // 3) Skip original change-points that lie within [before_start, before_end)
            while let Some((&line, _)) = cp_iter.peek().copied() {
                if line < before_end {
                    cp_iter.next();
                } else {
                    break;
                }
            }

            // 4) Resume cohort after the block, if there is a file after before_end
            if before_end < old_total {
                if let Some(resume_cohort) = self.cohort_at_index(before_end) {
                    push_cp(
                        (before_start as LineDelta + after_len as LineDelta + offset) as LineNumber,
                        resume_cohort,
                        &mut new_change_points,
                    );
                }
            }

            offset += delta;
        }

        // Emit the remaining original change-points after the last hunk, shifted by final offset
        while let Some((&line, &line_cohort)) = cp_iter.next() {
            push_cp(
                (line as LineDelta + offset) as LineNumber,
                line_cohort,
                &mut new_change_points,
            );
        }

        let new_total = (old_total as LineDelta + offset) as LineNumber;
        self.change_points = new_change_points;
        self.total_lines = new_total;
        self.merge_adjacent_ranges();
        // Drop any change-points that landed at or beyond new total
        // (can happen if a resume position coincides with the final end after deletions)
        if self.total_lines > 0 {
            let mut to_remove: Vec<LineNumber> = Vec::new();
            for &k in self.change_points.keys() {
                if k >= self.total_lines {
                    to_remove.push(k);
                }
            }
            for k in to_remove {
                self.change_points.remove(&k);
            }
        } else {
            self.change_points.clear();
        }
        self.validate().expect(
            format!(
                "invalid blame after applying line diffs: {:?}.\n old total: {:?}.\n offset: {:?}.\n line diffs: {:?}",
                self.total_lines(),
                old_total,
                offset.abs(),
                line_diffs
            )
            .as_str(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // A few hand-written tests. There are not very principled, I trust the PBT
    // implemented against the reference implementation to be more thorough.
    #[test]
    fn test_cohort_stats() {
        let mut blame = FileBlame::new(0, 2022);
        blame.apply_line_diffs(vec![(0..0, 0..10, 2022), (0..0, 5..10, 2023)]);

        let stats = blame.cohort_stats();
        assert_eq!(stats.get(&2022), Some(&10));
        assert_eq!(stats.get(&2023), Some(&5));
    }

    #[test]
    fn test_apply_line_diffs_equal_length_hunks_near_end() {
        let mut blame = FileBlame::new(160, 2000);

        // Multiple replacements near the end; total_lines must stay unchanged
        let diffs = vec![
            (46..49, 46..49, 2006),
            (65..66, 65..66, 2006),
            (92..95, 92..95, 2006),
            (99..100, 99..100, 2006),
            (104..105, 104..105, 2006),
            (106..107, 106..107, 2006),
            (109..110, 109..110, 2006),
            (111..113, 111..113, 2006),
            (138..145, 138..145, 2006),
            (146..149, 146..149, 2006),
            (153..154, 153..154, 2006),
        ];

        let old_total = blame.total_lines();
        blame.apply_line_diffs(diffs);
        assert_eq!(blame.total_lines(), old_total);
        blame.validate().unwrap();
    }

    #[test]
    fn test_apply_line_diffs_insertion_then_deletion_tail() {
        let mut blame = FileBlame::new(200, 1999);
        // Insert 5 lines at position 50
        blame.apply_line_diffs(vec![(50..50, 50..55, 2001)]);
        assert_eq!(blame.total_lines(), 205);
        blame.validate().unwrap();
        // Now delete last 10 lines (from position 195..205 -> 195..195)
        blame.apply_line_diffs(vec![(195..205, 195..195, 2002)]);
        assert_eq!(blame.total_lines(), 195);
        blame.validate().unwrap();
    }

    #[test]
    fn test_apply_line_diffs_insertion_and_followup_replacements() {
        let mut blame = FileBlame::new(150, 2015);
        // Insert 4 lines at 20
        blame.apply_line_diffs(vec![(20..20, 20..24, 2016)]);
        assert_eq!(blame.total_lines(), 154);
        // Multiple replacements later in the file
        let old_total = blame.total_lines();
        blame.apply_line_diffs(vec![(100..102, 100..102, 2017), (150..151, 150..151, 2017)]);
        assert_eq!(blame.total_lines(), old_total);
        blame.validate().unwrap();
    }

    #[test]
    fn test_apply_line_diffs_resume_would_land_at_final_end() {
        let mut blame = FileBlame::new(100, 1);
        // Hunk 1: simple replacement 90..95 -> 90..95 (delta 0)
        // Hunk 2: delete tail 95..100 -> 95..95 (delta -5)
        let diffs = vec![(90..95, 90..95, 2), (95..100, 95..95, 3)];
        blame.apply_line_diffs(diffs);
        assert_eq!(blame.total_lines(), 95);
        blame.validate().unwrap();
    }

    // Reference implementation that stores each line as an entry in a Vec.
    // So 3 lines of 2022 -> [2022, 2022, 2022]. Deleting 2 lines at pos X means
    // literally going to pos X and removing 2 items. No fancy bookkeeping.
    #[derive(Clone, Debug)]
    struct NaiveBlame<CohortKey: Keyable> {
        lines: Vec<CohortKey>,
    }

    impl<CohortKey: Keyable> NaiveBlame<CohortKey> {
        fn new(total_lines: LineNumber, cohort: CohortKey) -> Self {
            Self {
                lines: vec![cohort; total_lines as usize],
            }
        }

        fn total_lines(&self) -> LineNumber {
            self.lines.len() as LineNumber
        }

        fn apply_line_diffs(
            &mut self,
            mut line_diffs: Vec<(
                std::ops::Range<LineNumber>,
                std::ops::Range<LineNumber>,
                CohortKey,
            )>,
        ) {
            // Apply from bottom to top so indices of earlier hunks are unaffected
            line_diffs.sort_by_key(|(before, _, _)| before.start);
            line_diffs.reverse();
            for (before, after, cohort) in line_diffs.into_iter() {
                //TODO we should just throw here if given a range that is out of bounds
                let start = before.start.min(self.lines.len() as LineNumber) as usize;
                let before_len = (before.end - before.start) as usize;

                // delete
                if before_len > 0 {
                    let available = self.lines.len() - start;
                    let to_delete = before_len.min(available);
                    if to_delete > 0 {
                        self.lines.drain(start..start + to_delete);
                    }
                }
                // insert
                let after_len = (after.end - after.start) as usize;
                if after_len > 0 {
                    self.lines.splice(start..start, vec![cohort; after_len]);
                }
            }
        }

        fn expand(&self) -> Vec<CohortKey> {
            self.lines.clone()
        }
        fn cohort_stats(&self) -> std::collections::HashMap<CohortKey, u64> {
            let mut stats = std::collections::HashMap::new();
            for &cohort in self.lines.iter() {
                *stats.entry(cohort).or_insert(0) += 1;
            }
            stats
        }
    }

    fn expand_file_blame<C: Keyable + Eq>(fb: &FileBlame<C>) -> Vec<C> {
        let mut out: Vec<C> = Vec::with_capacity(fb.total_lines() as usize);
        for (start, end, cohort) in fb.ranges() {
            let count = end - start;
            for _ in 0..count {
                out.push(cohort);
            }
        }
        out
    }

    const FILE_START_LEN: LineNumber = 500;
    const NUM_COHORTS: u32 = 20;
    const TOTAL_GENERATED_HUNKS: usize = 1000;
    const BATCH_AVG_LEN: u32 = 5;

    proptest! {
        #[test]
        fn pbt_apply_line_diffs_matches_naive(
            initial_len in 0u32..FILE_START_LEN,
            initial_cohort in 0u32..NUM_COHORTS,
            ops in proptest::collection::vec((any::<u32>(), any::<u32>(), any::<u32>(), any::<u32>()), 0..TOTAL_GENERATED_HUNKS)
        ) {
            let mut fb: FileBlame<u32> = FileBlame::new(initial_len, initial_cohort);
            let mut naive: NaiveBlame<u32> = NaiveBlame::new(initial_len, initial_cohort);

            let mut pending: Vec<(std::ops::Range<u32>, std::ops::Range<u32>, u32)> = Vec::new();
            let mut current_len: u32;
            let mut batch_old_len: u32 = initial_len;
            let mut batch_last_end: u32 = 0;

            for (pos_seed, del_seed, ins_seed, cohort_seed) in ops.into_iter() {
                // Choose non-overlapping start positions within the snapshot (batch_old_len)
                let remaining_space = batch_old_len - batch_last_end;
                let gap = if remaining_space == 0 { 0 } else { pos_seed % (remaining_space + 1) };
                let position = batch_last_end + gap;
                // Deletion up to 7 lines, bounded by the snapshot length from position
                let max_del = batch_old_len - position;
                let before_len = if max_del == 0 { 0 } else { del_seed % (max_del + 1) };
                // Insertion up to 7 lines
                let after_len = ins_seed % 8;
                let cohort = (cohort_seed % 11) + 1000; // avoid 0 to reduce accidental equality with prev cohort

                let before = position..(position + before_len);
                let after = position..(position + after_len);
                pending.push((before, after, cohort));
                batch_last_end = position + before_len; // enforce non-overlap and ascending order

                if pos_seed % BATCH_AVG_LEN == 0 {
                    fb.apply_line_diffs(pending.clone());
                    naive.apply_line_diffs(pending.clone());
                    pending.clear();

                    current_len = naive.total_lines();
                    prop_assert_eq!(fb.total_lines(), current_len);

                    let fb_lines = expand_file_blame(&fb);
                    let naive_lines = naive.expand();
                    prop_assert_eq!(fb_lines, naive_lines);

                    prop_assert_eq!(fb.cohort_stats(), naive.cohort_stats());

                    batch_old_len = current_len;
                    batch_last_end = 0;
                }
            }

            if !pending.is_empty() {
                fb.apply_line_diffs(pending.clone());
                naive.apply_line_diffs(pending.clone());
            }

            prop_assert_eq!(fb.total_lines(), naive.total_lines());
            let fb_lines = expand_file_blame(&fb);
            let naive_lines = naive.expand();
            prop_assert_eq!(fb_lines, naive_lines);
            prop_assert!(fb.validate().is_ok());
            prop_assert_eq!(fb.cohort_stats(), naive.cohort_stats());
        }
    }
}
