use std::{
    collections::BTreeMap,
    fmt::{Debug, Display},
};

type LineNumber = u32;

pub trait Keyable: Copy + PartialEq + Display + Debug {}
impl<T: Copy + PartialEq + Display + Debug> Keyable for T {}

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

    pub fn blame_for_line(&self, line: LineNumber) -> Option<BlameRange<CohortKey>> {
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
        Some(BlameRange::new(start, next_end - start, cohort))
    }

    pub fn insert_lines(
        &mut self,
        position: LineNumber,
        line_count: LineNumber,
        cohort: CohortKey,
    ) {
        self.insert_lines_without_merge(position, line_count, cohort);
        self.merge_adjacent_ranges();
    }

    pub fn insert_lines_without_merge(
        &mut self,
        mut position: LineNumber,
        line_count: LineNumber,
        cohort: CohortKey,
    ) {
        if line_count == 0 {
            return;
        }
        if position > self.total_lines {
            position = self.total_lines;
        }
        let old_total = self.total_lines;

        let old_at_pos = if position < self.total_lines {
            self.cohort_at_index(position)
        } else {
            self.change_points.iter().next_back().map(|(_, &v)| v)
        };

        // Split tail at position to avoid remove/insert churn
        let tail = self.change_points.split_off(&position); // keys >= position
        let mut new_tail: BTreeMap<LineNumber, CohortKey> = BTreeMap::new();
        for (k, v) in tail.into_iter() {
            new_tail.insert(k + line_count, v);
        }

        // If the existing cohort at position already equals the new cohort, we only need to shift the tail
        match old_at_pos {
            Some(old) if old == cohort => {
                // nothing to insert at position or resume; the existing segment extends
            }
            Some(old) => {
                // Insert new cohort at position
                self.change_points.insert(position, cohort);
                // Restore previous cohort after the inserted block if needed
                let resume_pos = position + line_count;
                // Only resume if insertion is not strictly at the end of the file.
                // If we insert at EOF, there is no content to resume, and adding a
                // change-point at new total_lines would be invalid.
                if position < old_total {
                    // Check whether the first key in new_tail already resumes with the same cohort
                    let already_resumes = new_tail
                        .range(resume_pos..=resume_pos)
                        .next()
                        .map(|(_, &v)| v == old)
                        .unwrap_or(false);
                    if !already_resumes {
                        self.change_points.insert(resume_pos, old);
                    }
                }
            }
            None => {
                // inserting at end on empty file or brand new tail
                // Only add a position change-point if last cohort differs
                if self.change_points.iter().next_back().map(|(_, &v)| v) != Some(cohort) {
                    self.change_points.insert(position, cohort);
                }
            }
        }

        // Append shifted tail back
        self.change_points.append(&mut new_tail);

        self.total_lines += line_count;
    }

    pub fn delete_lines(&mut self, position: u32, line_count: u32) {
        self.delete_lines_without_merge(position, line_count);
        self.merge_adjacent_ranges();
    }

    pub fn delete_lines_without_merge(&mut self, position: u32, line_count: u32) {
        if line_count == 0 || position >= self.total_lines {
            return;
        }
        let end_position = std::cmp::min(position + line_count, self.total_lines);
        if end_position == position {
            return;
        }
        let delta = end_position - position;

        // Cohort at end_position (becomes the cohort at `position` after deletion)
        let after_end = if end_position < self.total_lines {
            self.cohort_at_index(end_position)
        } else {
            None
        };

        // Fast path: delete to end
        if end_position == self.total_lines {
            // Remove [position, end)
            let _mid = self.change_points.split_off(&position);
            // If nothing remains, we are done
            self.total_lines -= delta;
            return;
        }

        // Split tail >= end_position
        let tail = self.change_points.split_off(&end_position); // keys >= end
                                                                // Remove middle [position, end)
        let _mid = self.change_points.split_off(&position); // drop mid keys

        // Shift tail by -delta
        let mut new_tail: BTreeMap<LineNumber, CohortKey> = BTreeMap::new();
        for (k, v) in tail.into_iter() {
            new_tail.insert(k - delta, v);
        }

        // Append shifted tail back
        self.change_points.append(&mut new_tail);

        // Ensure cohort at `position` matches `after_end` (avoid redundant keys)
        let new_total = self.total_lines - delta;
        if position < new_total {
            if let Some(v) = after_end {
                let current = self.cohort_at_index(position);
                if current != Some(v) {
                    // Also avoid inserting if the immediately previous key already has v
                    let prev_same = self
                        .change_points
                        .range(..=position)
                        .next_back()
                        .map(|(_, &pv)| pv == v)
                        .unwrap_or(false);
                    if !prev_same {
                        self.change_points.insert(position, v);
                    }
                }
            }
        } else if position == 0 && new_total == 0 {
            self.change_points.clear();
        }

        self.total_lines = new_total;
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

    /// Get an iterator over all blame ranges
    pub fn ranges(&self) -> impl Iterator<Item = BlameRange<CohortKey>> + '_ {
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
                Some(BlameRange::new(start, end - start, cohort))
            } else {
                None
            }
        })
    }

    pub fn cohort_stats(&self) -> std::collections::HashMap<CohortKey, u64>
    where
        CohortKey: Eq + std::hash::Hash,
    {
        let mut stats = std::collections::HashMap::new();
        for range in self.ranges() {
            *stats.entry(range.cohort).or_insert(0) += range.line_count as u64;
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
            Some(&k) if k == 0 => {}
            _ => return Err("The first change-point must be at line 0".to_string()),
        }
        // Keys must be strictly increasing and strictly less than total_lines
        let mut prev_key: Option<LineNumber> = None;
        for &k in self.change_points.keys() {
            if k >= self.total_lines {
                return Err("Change-point key must be < total_lines".to_string());
            }
            if let Some(prev) = prev_key {
                if k <= prev {
                    return Err("Change-point keys must be strictly increasing".to_string());
                }
            }
            prev_key = Some(k);
        }
        Ok(())
    }

    pub fn apply_line_diffs(
        &mut self,
        line_diffs: Vec<(std::ops::Range<u32>, std::ops::Range<u32>, CohortKey)>,
    ) {
        if line_diffs.is_empty() {
            return;
        }
        // Bulk-apply: rebuild change-points in a single pass for speed
        let mut diffs = line_diffs.clone();
        diffs.sort_by_key(|(before, _, _)| before.start);

        let old_total = self.total_lines;
        let mut new_change_points: BTreeMap<LineNumber, CohortKey> = BTreeMap::new();
        let mut cp_iter = self.change_points.iter().peekable();
        let mut offset: i64 = 0;

        // Helper to push a change point only if cohort changes
        let push_cp = |pos: u32, cohort: CohortKey, map: &mut BTreeMap<u32, CohortKey>| {
            if let Some((_, &last_v)) = map.last_key_value() {
                if last_v == cohort {
                    return;
                }
            }
            map.insert(pos, cohort);
        };

        for (before, after, cohort) in diffs.into_iter() {
            let b0 = before.start;
            let b1 = before.end;
            let alen = after.len() as u32;
            let blen = b1 - b0;
            let delta = alen as i64 - blen as i64;

            // 1) Emit unaffected change-points before b0, shifted by current offset
            while let Some((&k, &v)) = cp_iter.peek().copied() {
                if k < b0 {
                    let new_k = (k as i64 + offset) as u32;
                    push_cp(new_k, v, &mut new_change_points);
                    cp_iter.next();
                } else {
                    break;
                }
            }

            // 2) Insert the new block's cohort at b0 if any insertion
            if alen > 0 {
                let ins_pos_new = (b0 as i64 + offset) as u32;
                push_cp(ins_pos_new, cohort, &mut new_change_points);
            }

            // 3) Skip original change-points that lie within [b0, b1)
            while let Some((&k, _)) = cp_iter.peek().copied() {
                if k < b1 {
                    cp_iter.next();
                } else {
                    break;
                }
            }

            // 4) Resume cohort after the block, if there is a file after b1
            if b1 < old_total {
                if let Some(resume_cohort) = self.cohort_at_index(b1) {
                    let resume_pos_new = (b0 as i64 + alen as i64 + offset) as u32;
                    push_cp(resume_pos_new, resume_cohort, &mut new_change_points);
                }
            }

            // 5) Update offset
            offset += delta;
        }

        // Emit the remaining original change-points after the last hunk, shifted by final offset
        while let Some((k, v)) = cp_iter.next() {
            let new_k = (*k as i64 + offset) as u32;
            push_cp(new_k, *v, &mut new_change_points);
        }

        // Update structure
        let new_total = (old_total as i64 + offset) as u32;
        self.change_points = new_change_points;
        self.total_lines = new_total;
        self.merge_adjacent_ranges();
        // Drop any change-points that erroneously landed at or beyond new total
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

    pub fn update(&mut self, position: u32, insert_len: u32, delete_len: u32, cohort: CohortKey) {
        self.update_without_merge(position, insert_len, delete_len, cohort);
        self.merge_adjacent_ranges();
    }

    /// Same as `update`, but defers merging of adjacent ranges for batch efficiency.
    pub fn update_without_merge(
        &mut self,
        position: u32,
        insert_len: u32,
        delete_len: u32,
        cohort: CohortKey,
    ) {
        if insert_len == 0 && delete_len == 0 {
            return;
        }

        if delete_len > 0 {
            self.delete_lines_without_merge(position, delete_len);
        }

        if insert_len > 0 {
            self.insert_lines_without_merge(position, insert_len, cohort);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_cohort() -> u32 {
        2023
    }

    #[test]
    fn test_new_file_blame() {
        let cohort = create_test_cohort();
        let blame = FileBlame::new(10, cohort);

        assert_eq!(blame.total_lines(), 10);
        assert_eq!(blame.range_count(), 1);

        let range = blame.blame_for_line(5).unwrap();
        assert_eq!(range.start_line, 0);
        assert_eq!(range.line_count, 10);
        assert_eq!(range.cohort, cohort);
    }

    #[test]
    fn test_insert_lines() {
        let cohort = create_test_cohort();
        let mut blame = FileBlame::new(10, cohort);

        let new_cohort = 2024;

        blame.insert_lines(5, 3, new_cohort);

        assert_eq!(blame.range_count(), 3);
        assert_eq!(blame.total_lines(), 13);

        assert_eq!(blame.blame_for_line(4).unwrap().cohort, cohort);
        assert_eq!(blame.blame_for_line(5).unwrap().cohort, new_cohort);
        assert_eq!(blame.blame_for_line(7).unwrap().cohort, new_cohort);
        assert_eq!(blame.blame_for_line(8).unwrap().cohort, cohort);

        blame.validate().unwrap();
    }

    #[test]
    fn test_delete_lines() {
        let cohort = create_test_cohort();
        let mut blame = FileBlame::new(10, cohort);

        blame.delete_lines(3, 3);

        assert_eq!(blame.range_count(), 1);
        assert_eq!(blame.total_lines(), 7);

        blame.validate().unwrap();
    }

    #[test]
    fn test_cohort_stats() {
        let mut blame = FileBlame::new(10, 2022);
        blame.insert_lines(5, 5, 2023);

        let stats = blame.cohort_stats();
        assert_eq!(stats.get(&2022), Some(&10));
        assert_eq!(stats.get(&2023), Some(&5));
    }

    #[test]
    fn test_delete_from_beginning() {
        let mut blame = FileBlame::new(10, 2022);
        blame.delete_lines(0, 3);

        assert_eq!(blame.total_lines(), 7);
        assert_eq!(blame.range_count(), 1);

        let range = blame.blame_for_line(0).unwrap();
        assert_eq!(range.start_line, 0);
        assert_eq!(range.line_count, 7);
        assert_eq!(range.cohort, 2022);

        blame.validate().unwrap();
    }

    #[test]
    fn test_delete_from_end() {
        let mut blame = FileBlame::new(10, 2022);

        blame.delete_lines(7, 3);

        assert_eq!(blame.range_count(), 1);
        assert_eq!(blame.total_lines(), 7);

        let range = blame.blame_for_line(6).unwrap();
        assert_eq!(range.cohort, 2022);
        assert_eq!(range.start_line, 0);
        assert_eq!(range.line_count, 7);

        blame.validate().unwrap();
    }

    #[test]
    fn test_delete_all_lines() {
        let mut blame = FileBlame::new(10, 2022);
        blame.delete_lines(0, 10);

        assert_eq!(blame.range_count(), 0);
        assert_eq!(blame.total_lines(), 0);
        blame.validate().unwrap();
    }

    #[test]
    fn test_insert_at_beginning() {
        let mut blame = FileBlame::new(10, 2022);
        blame.insert_lines(0, 5, 2024);

        assert_eq!(blame.total_lines(), 15);
        assert_eq!(blame.range_count(), 2);

        // Check first range (new lines)
        let range = blame.blame_for_line(0).unwrap();
        assert_eq!(range.start_line, 0);
        assert_eq!(range.line_count, 5);
        assert_eq!(range.cohort, 2024);

        // Check second range (original lines)
        let range = blame.blame_for_line(5).unwrap();
        assert_eq!(range.start_line, 5);
        assert_eq!(range.line_count, 10);
        assert_eq!(range.cohort, 2022);

        blame.validate().unwrap();
    }

    #[test]
    fn test_insert_at_end() {
        let mut blame = FileBlame::new(10, 2022);
        blame.insert_lines(10, 5, 2024);

        assert_eq!(blame.total_lines(), 15);
        assert_eq!(blame.range_count(), 2);

        // Check first range (original lines)
        let range = blame.blame_for_line(0).unwrap();
        assert_eq!(range.start_line, 0);
        assert_eq!(range.line_count, 10);
        assert_eq!(range.cohort, 2022);

        // Check second range (new lines)
        let range = blame.blame_for_line(10).unwrap();
        assert_eq!(range.start_line, 10);
        assert_eq!(range.line_count, 5);
        assert_eq!(range.cohort, 2024);

        blame.validate().unwrap();
    }

    #[test]
    fn test_fused_insert_delete() {
        let mut blame = FileBlame::new(10, 2022);

        blame.insert_lines(5, 3, 2024);
        // Then delete 2 lines from the inserted section
        blame.delete_lines(6, 2);

        assert_eq!(blame.total_lines(), 11);
        assert_eq!(blame.range_count(), 3);

        blame.validate().unwrap();
    }

    #[test]
    fn test_delete_same_beginning_pattern() {
        let mut blame = FileBlame::new(100, 2022);

        blame.insert_lines(0, 5, 2024);

        // Should have 2 ranges: [0-4] with new_cohort, [5-104] with original cohort
        assert_eq!(blame.total_lines(), 105);
        assert_eq!(blame.range_count(), 2);

        // Delete the 5 inserted lines from beginning
        blame.delete_lines(0, 5);

        // Should merge back to single range since remaining lines have same metadata
        assert_eq!(blame.total_lines(), 100);
        assert_eq!(blame.range_count(), 1);

        let range = blame.blame_for_line(0).unwrap();
        assert_eq!(range.start_line, 0);
        assert_eq!(range.line_count, 100);
        assert_eq!(range.cohort, 2022);

        blame.validate().unwrap();
    }

    #[test]
    fn test_apply_line_diffs_equal_length_hunks_near_end() {
        let mut blame = FileBlame::new(160, 2000);

        // multiple replacement hunks (alen == blen) near the end; total_lines must stay unchanged
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
        let diffs_insert = vec![(50..50, 50..55, 2001)];
        blame.apply_line_diffs(diffs_insert);
        assert_eq!(blame.total_lines(), 205);
        blame.validate().unwrap();

        // Now delete last 10 lines (from position 195..205 -> 195..195)
        let diffs_delete_tail = vec![(195..205, 195..195, 2002)];
        blame.apply_line_diffs(diffs_delete_tail);
        assert_eq!(blame.total_lines(), 195);
        blame.validate().unwrap();
    }

    #[test]
    fn test_apply_line_diffs_mid_deletion_creates_resume_before_end() {
        let mut blame = FileBlame::new(120, 2010);

        // Create a second cohort mid-file to ensure resume logic chooses correct cohort
        blame.insert_lines(60, 0, 2011); // no-op insert but ensures cohort boundary at 60
        blame.insert_lines(60, 1, 2011); // actually insert 1 line to create a split
        blame.delete_lines(60, 1); // revert split while keeping internal structure exercised

        // Delete lines in the middle: 40..50
        let diffs_delete_mid = vec![(40..50, 40..40, 2012)];
        let old_total = blame.total_lines();
        blame.apply_line_diffs(diffs_delete_mid);
        assert_eq!(blame.total_lines(), old_total - 10);
        // No change-point should be at or beyond total_lines
        blame.validate().unwrap();
    }

    #[test]
    fn test_apply_line_diffs_insertion_and_followup_replacements() {
        let mut blame = FileBlame::new(150, 2015);

        // Insert 4 lines at 20
        let diffs_insert = vec![(20..20, 20..24, 2016)];
        blame.apply_line_diffs(diffs_insert);
        assert_eq!(blame.total_lines(), 154);

        // Follow with multiple equal-length replacements later in the file
        let diffs_replace = vec![(100..102, 100..102, 2017), (150..151, 150..151, 2017)];
        let old_total = blame.total_lines();
        blame.apply_line_diffs(diffs_replace);
        assert_eq!(blame.total_lines(), old_total);
        blame.validate().unwrap();
    }

    #[test]
    fn test_apply_line_diffs_resume_would_land_at_final_end() {
        // Earlier hunk resumes at position X, later hunk deletes tail starting at X,
        // so final total becomes exactly X. Resume at X must not be inserted.
        let mut blame = FileBlame::new(100, 1);

        // Hunk 1: equal-length replacement 90..95 -> 90..95 (delta 0)
        // Hunk 2: delete tail 95..100 -> 95..95 (delta -5)
        let diffs = vec![(90..95, 90..95, 2), (95..100, 95..95, 3)];
        blame.apply_line_diffs(diffs);
        assert_eq!(blame.total_lines(), 95);
        // Ensure no change-point at or beyond 95
        blame.validate().unwrap();
    }
}
