use std::collections::BTreeMap;

type LineNumber = u32;
/// Represents blame information for a single line range in a file
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlameRange<CohortKey>
where
    CohortKey: Copy + PartialEq,
{
    /// Starting line number (0-based)
    pub start_line: LineNumber,
    /// Number of lines in this range
    pub line_count: LineNumber,
    /// Cohort identifier (e.g., 2023 for yearly cohorts)
    pub cohort: CohortKey,
}

impl<CohortKey: Copy + PartialEq> BlameRange<CohortKey> {
    pub fn new(start_line: LineNumber, line_count: LineNumber, cohort: CohortKey) -> Self {
        Self {
            start_line,
            line_count,
            cohort,
        }
    }

    /// Get the ending line number (exclusive)
    pub fn end_line(&self) -> LineNumber {
        self.start_line + self.line_count
    }

    /// Check if this range overlaps with another range
    pub fn overlaps_with(&self, other: &BlameRange<CohortKey>) -> bool {
        self.start_line < other.end_line() && other.start_line < self.end_line()
    }

    pub fn contains_line(&self, line: LineNumber) -> bool {
        line >= self.start_line && line < self.end_line()
    }
}

#[derive(Debug, Clone)]
pub struct FileBlame<CohortKey>
where
    CohortKey: Copy + PartialEq,
{
    /// Map from start_line to BlameRange
    /// BTreeMap provides O(log n) lookups and maintains sorted order
    ranges: BTreeMap<LineNumber, BlameRange<CohortKey>>,
    /// Total number of lines in the file
    total_lines: LineNumber,
}

impl<CohortKey> FileBlame<CohortKey>
where
    CohortKey: Copy + PartialEq,
{
    /// Create a new FileBlame for a file with the given number of lines
    /// Initially, all lines belong to a single cohort
    pub fn new(total_lines: LineNumber, cohort: CohortKey) -> Self {
        let mut ranges = BTreeMap::new();
        if total_lines > 0 {
            let range = BlameRange::new(0, total_lines, cohort);
            ranges.insert(0, range);
        }

        Self {
            ranges,
            total_lines,
        }
    }

    /// Create an empty FileBlame
    pub fn empty() -> Self {
        Self {
            ranges: BTreeMap::new(),
            total_lines: 0,
        }
    }

    pub fn total_lines(&self) -> LineNumber {
        self.total_lines
    }

    pub fn range_count(&self) -> usize {
        self.ranges.len()
    }

    pub fn blame_for_line(&self, line: LineNumber) -> Option<&BlameRange<CohortKey>> {
        // Find the range with the largest start_line <= line
        self.ranges
            .range(..=line)
            .next_back()
            .and_then(|(_, range)| {
                if range.contains_line(line) {
                    Some(range)
                } else {
                    None
                }
            })
    }

    /// Update the blame information when lines are inserted
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
        position: LineNumber,
        line_count: LineNumber,
        cohort: CohortKey,
    ) {
        if line_count == 0 {
            return;
        }

        let mut ranges_to_modify = Vec::new();
        let mut ranges_to_add = Vec::new();

        // Collect ranges that need modification
        // First, check if there's a range that contains the insertion position
        if let Some((&start_line, range)) = self.ranges.range(..=position).next_back() {
            if range.contains_line(position) {
                ranges_to_modify.push(start_line);
            }
        }

        // Then collect all ranges that start at or after the insertion position
        ranges_to_modify.extend(
            self.ranges
                .range(position..)
                .map(|(&start_line, _)| start_line),
        );

        let inserting_at_end = ranges_to_modify.is_empty();

        // Process the modifications
        for start_line in ranges_to_modify {
            if let Some(mut range) = self.ranges.remove(&start_line) {
                if range.contains_line(position) {
                    // Split this range
                    if range.start_line < position {
                        // Keep the part before the insertion
                        let before_range = BlameRange::new(
                            range.start_line,
                            position - range.start_line,
                            range.cohort,
                        );
                        ranges_to_add.push(before_range);
                    }

                    // Add the new inserted lines
                    let new_range = BlameRange::new(position, line_count, cohort);
                    ranges_to_add.push(new_range);

                    // Add the part after the insertion (shifted)
                    if position < range.end_line() {
                        let after_range = BlameRange::new(
                            position + line_count,
                            range.end_line() - position,
                            range.cohort,
                        );
                        ranges_to_add.push(after_range);
                    }
                } else {
                    // Just shift this range
                    range.start_line += line_count;
                    ranges_to_add.push(range);
                }
            }
        }

        // If no ranges were modified (e.g., inserting at the end of the file),
        // we still need to add the new range
        if inserting_at_end {
            let new_range = BlameRange::new(position, line_count, cohort);
            ranges_to_add.push(new_range);
        }

        for range in ranges_to_add {
            self.ranges.insert(range.start_line, range);
        }

        self.total_lines += line_count;
    }

    /// Update the blame information when lines are deleted
    pub fn delete_lines(&mut self, position: u32, line_count: u32) {
        self.delete_lines_without_merge(position, line_count);
        self.merge_adjacent_ranges();
    }

    pub fn delete_lines_without_merge(&mut self, position: u32, line_count: u32) {
        if line_count == 0 || position >= self.total_lines {
            return;
        }

        let end_position = std::cmp::min(position + line_count, self.total_lines);
        let mut ranges_to_modify = Vec::new();
        let mut ranges_to_add = Vec::new();

        // Collect ranges that are affected by the deletion
        // Find ranges that overlap with the deletion region [position, end_position)
        ranges_to_modify.extend(self.ranges.range(..end_position).filter_map(
            |(&start_line, range)| {
                if range.start_line < end_position && range.end_line() > position {
                    Some(start_line)
                } else {
                    None
                }
            },
        ));

        // Add ranges that start at or after end_position (these just need shifting)
        ranges_to_modify.extend(
            self.ranges
                .range(end_position..)
                .map(|(&start_line, _)| start_line),
        );

        // Process the modifications
        for start_line in ranges_to_modify {
            if let Some(mut range) = self.ranges.remove(&start_line) {
                if range.start_line < end_position && range.end_line() > position {
                    // This range is affected by the deletion
                    if range.start_line < position {
                        // Keep the part before the deletion
                        let before_range = BlameRange::new(
                            range.start_line,
                            position - range.start_line,
                            range.cohort,
                        );
                        ranges_to_add.push(before_range);
                    }

                    if range.end_line() > end_position {
                        // Keep the part after the deletion (shifted)
                        let after_range = BlameRange::new(
                            position,
                            range.end_line() - end_position,
                            range.cohort,
                        );
                        ranges_to_add.push(after_range);
                    }
                } else if range.start_line >= end_position {
                    // Just shift this range
                    range.start_line -= line_count;
                    ranges_to_add.push(range);
                }
            }
        }

        // Add all the modified ranges
        for range in ranges_to_add {
            self.ranges.insert(range.start_line, range);
        }

        self.total_lines -= line_count;
    }

    /// Merge adjacent ranges that have the same blame information
    /// This is an optimization to reduce memory usage
    pub fn merge_adjacent_ranges(&mut self) {
        let mut new_ranges = BTreeMap::new();
        let mut current_range: Option<BlameRange<CohortKey>> = None;

        for (_, range) in std::mem::take(&mut self.ranges) {
            match &mut current_range {
                None => {
                    current_range = Some(range);
                }
                Some(current) => {
                    if current.end_line() == range.start_line && current.cohort == range.cohort {
                        current.line_count += range.line_count;
                    } else {
                        new_ranges.insert(current.start_line, *current);
                        current_range = Some(range);
                    }
                }
            }
        }
        if let Some(range) = current_range {
            new_ranges.insert(range.start_line, range);
        }

        self.ranges = new_ranges;
    }

    /// Get an iterator over all blame ranges
    pub fn ranges(&self) -> impl Iterator<Item = &BlameRange<CohortKey>> {
        self.ranges.values()
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
        let mut expected_line = 0;

        for range in self.ranges() {
            if range.start_line != expected_line {
                return Err(format!(
                    "Gap or overlap detected: expected line {}, found range starting at {}",
                    expected_line, range.start_line
                ));
            }
            if range.line_count == 0 {
                return Err("Found range with zero line count".to_string());
            }
            expected_line = range.end_line();
        }

        if expected_line != self.total_lines {
            return Err(format!(
                "Total lines mismatch: ranges cover {} lines but file has {} lines",
                expected_line, self.total_lines
            ));
        }

        Ok(())
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
}
