use std::collections::HashSet;

const MARKERS: [&str; 5] = ["featuring ", "feat. ", "feat ", "ft. ", "ft "];

fn names_start(text: &str) -> Option<usize> {
    let lower = text.to_ascii_lowercase();
    let trimmed = lower.trim_start();
    let blanks = lower.len() - trimmed.len();
    MARKERS
        .iter()
        .find(|marker| trimmed.starts_with(*marker))
        .map(|marker| blanks + marker.len())
}

fn clause_names(clauses: Vec<&str>) -> HashSet<String> {
    let mut names = HashSet::new();
    for clause in clauses {
        let clause = clause.trim().to_lowercase();
        names.extend(
            clause
                .replace(" & ", ",")
                .replace(" and ", ",")
                .split(',')
                .map(|name| name.trim().to_owned())
                .filter(|name| !name.is_empty()),
        );
        names.insert(clause);
    }
    names
}

pub fn bracketed_feature_names(title: &str) -> HashSet<String> {
    let mut clauses = Vec::new();
    for (open, close) in [('(', ')'), ('[', ']')] {
        for (start, _) in title.match_indices(open) {
            let inner = &title[start + 1..];
            if let (Some(from), Some(end)) = (names_start(inner), inner.find(close))
                && from <= end
            {
                clauses.push(&inner[from..end]);
            }
        }
    }
    for (start, _) in title.match_indices(" - ") {
        let rest = &title[start + 3..];
        if let Some(from) = names_start(rest) {
            clauses.push(&rest[from..]);
        }
    }
    clause_names(clauses)
}

pub fn credit_feature_names(credit: &str) -> HashSet<String> {
    let mut names = bracketed_feature_names(credit);
    let lower = credit.to_ascii_lowercase();
    let bare = MARKERS
        .iter()
        .filter_map(|marker| {
            lower
                .find(&format!(" {marker}"))
                .map(|at| at + 1 + marker.len())
        })
        .min();
    if let Some(from) = bare {
        let rest = &credit[from..];
        let end = rest.find([')', ']']).unwrap_or(rest.len());
        names.extend(clause_names(vec![&rest[..end]]));
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credit_names_featured_artists_after_its_marker() {
        let names = credit_feature_names("Ada Feat. Bo & Cy");
        assert!(names.contains("bo") && names.contains("cy") && names.contains("bo & cy"));
        assert!(!names.contains("ada"));
        assert!(credit_feature_names("Ada (ft. Bo)").contains("bo"));
        assert!(credit_feature_names("Ada & Bo").is_empty());
        assert!(credit_feature_names("Soft Machine").is_empty());
    }
}
