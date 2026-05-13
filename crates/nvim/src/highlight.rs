const MAX_DIFF_CELLS: usize = 1_000_000;

pub fn changed_line_indexes(current: &str, reference: &str) -> Vec<usize> {
    let current_lines: Vec<&str> = current.lines().collect();
    let reference_lines: Vec<&str> = reference.lines().collect();

    if current_lines
        .len()
        .checked_mul(reference_lines.len())
        .is_none_or(|cells| cells > MAX_DIFF_CELLS)
    {
        return positional_changed_line_indexes(&current_lines, &reference_lines);
    }

    let common = lcs_line_pairs(&current_lines, &reference_lines);
    let mut unchanged = vec![false; current_lines.len()];

    for (current_index, _) in common {
        unchanged[current_index] = true;
    }

    unchanged
        .into_iter()
        .enumerate()
        .filter_map(|(index, is_unchanged)| (!is_unchanged).then_some(index))
        .collect()
}

fn positional_changed_line_indexes(current: &[&str], reference: &[&str]) -> Vec<usize> {
    current
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (reference.get(index).copied() != Some(*line)).then_some(index))
        .collect()
}

fn lcs_line_pairs(left: &[&str], right: &[&str]) -> Vec<(usize, usize)> {
    let mut lengths = vec![vec![0; right.len() + 1]; left.len() + 1];

    for (left_index, left_line) in left.iter().enumerate() {
        for (right_index, right_line) in right.iter().enumerate() {
            lengths[left_index + 1][right_index + 1] = if left_line == right_line {
                lengths[left_index][right_index] + 1
            } else {
                lengths[left_index + 1][right_index].max(lengths[left_index][right_index + 1])
            };
        }
    }

    let mut pairs = Vec::new();
    let mut left_index = left.len();
    let mut right_index = right.len();

    while left_index > 0 && right_index > 0 {
        if left[left_index - 1] == right[right_index - 1] {
            pairs.push((left_index - 1, right_index - 1));
            left_index -= 1;
            right_index -= 1;
        } else if lengths[left_index - 1][right_index] >= lengths[left_index][right_index - 1] {
            left_index -= 1;
        } else {
            right_index -= 1;
        }
    }

    pairs.reverse();
    pairs
}

#[cfg(test)]
mod tests {
    use super::changed_line_indexes;

    #[test]
    fn returns_modified_lines() {
        let current = "one\ntwo changed\nthree";
        let reference = "one\ntwo\nthree";

        assert_eq!(changed_line_indexes(current, reference), vec![1]);
    }

    #[test]
    fn ignores_inserted_reference_lines() {
        let current = "one\nthree";
        let reference = "one\ntwo\nthree";

        assert!(changed_line_indexes(current, reference).is_empty());
    }

    #[test]
    fn marks_inserted_current_lines() {
        let current = "one\ntwo\nthree";
        let reference = "one\nthree";

        assert_eq!(changed_line_indexes(current, reference), vec![1]);
    }
}
