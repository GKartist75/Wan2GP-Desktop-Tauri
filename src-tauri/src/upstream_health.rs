//! Upstream health: commits on Wan2GP `main` that are known to break the app
//! while upstream has not shipped a fix yet.
//!
//! `install::update` resets the checkout to `FETCH_HEAD` (upstream `main`) with
//! no stash, so a broken upstream commit becomes every launcher user's problem
//! the moment they press Update — and the only recovery is a manual rollback.
//! This table lets the update stop *before* applying anything and name the
//! issue, with "update anyway" one dialog away.
//!
//! Rules for adding an entry:
//! - the bug must be reproducible on a launcher install and confirmed upstream
//!   (link the issue), and
//! - the fix must be absent from `main` (check that before adding; delete the
//!   entry once it merges — this is a stop-gap, not a changelog).
//!
//! Missing an entry is safe (the update just proceeds). A stale entry only
//! costs one extra confirmation dialog, so the bar for removal is "merged".

/// One known-bad upstream commit.
pub struct KnownBad {
    /// Short (7+ hex) commit hash as it appears in `git log --oneline`.
    pub sha: &'static str,
    /// One line: what breaks, for someone who has never read the issue.
    pub title: &'static str,
    /// Upstream issue number (rendered as `deepbeepmeep/WanGP#<issue>`).
    pub issue: u32,
    /// What the user can do instead.
    pub advice: &'static str,
}

/// Curated stop-gap list. See the module docs for the add/remove rules.
pub const KNOWN_BAD: &[KnownBad] = &[KnownBad {
    // SeedVC's CAMPPlus model is the only towerless one in that pipeline, so
    // `towers_total / total_size` is 0/0 inside the "Async loading plan" print
    // in mmgp/offload.py's tune_preloading(). Introduced by 2c67e50 (VRAM
    // estimation fix); the one-line guard is PR #2548, unmerged as of
    // 2026-10-27 and re-confirmed on a Wan2GP Desktop install.
    sha: "2c67e50",
    title: "SeedVC crashes with ZeroDivisionError in mmgp offload",
    issue: 2543,
    advice: "Stay on the current commit until upstream merges PR #2548 (one-line guard).",
}];

/// Known-bad entries present in `commits` (full hashes).
///
/// Short-hash prefix matching is what `git log --oneline` shows and what the
/// table stores, so a commit is "present" when its full hash starts with the
/// table's short sha. Order follows the table, so the UI is stable.
pub fn known_bad_in(commits: &[String]) -> Vec<&'static KnownBad> {
    KNOWN_BAD
        .iter()
        .filter(|bad| {
            commits
                .iter()
                .any(|c| c.len() >= bad.sha.len() && c.starts_with(bad.sha))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full(short: &str) -> String {
        format!("{short}{}", "0".repeat(40 - short.len()))
    }

    #[test]
    fn matches_the_bad_commit_inside_the_incoming_range() {
        let commits = vec![full("1111111"), full("2c67e50")];
        let hits = known_bad_in(&commits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].sha, "2c67e50");
        assert_eq!(hits[0].issue, 2543);
    }

    #[test]
    fn ignores_unrelated_commits() {
        assert!(known_bad_in(&[full("97845dc"), full("9905f0d")]).is_empty());
        assert!(known_bad_in(&[]).is_empty());
    }

    #[test]
    fn prefix_must_be_a_prefix_not_a_substring() {
        // A hash that merely contains the short sha is not the commit.
        let mut other = String::from("fffffff");
        other.push_str("2c67e50");
        assert!(known_bad_in(&[other]).is_empty());
        // Shorter than the table entry can never match.
        assert!(known_bad_in(&["2c67e".to_string()]).is_empty());
    }

    #[test]
    fn table_entries_are_well_formed() {
        for bad in KNOWN_BAD {
            assert!(
                (7..=40).contains(&bad.sha.len()),
                "{}: sha should be a 7-40 char git prefix",
                bad.sha
            );
            assert!(bad.sha.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(!bad.title.is_empty());
            assert!(bad.issue > 0);
            assert!(!bad.advice.is_empty());
        }
    }
}
