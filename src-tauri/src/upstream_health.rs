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
///
/// Empty since 2026-10-30: upstream 86671f15 added the `total_size` guard in
/// mmgp/offload.py, so the SeedVC ZeroDivisionError (#2543, reported against
/// 2c67e50) no longer reproduces from `main`. PR #2548 is still open, but the
/// fix itself is in the tree; the entry was removed under the "merged" rule.
pub const KNOWN_BAD: &[KnownBad] = &[];

/// Known-bad entries present in `commits` (full hashes).
///
/// Short-hash prefix matching is what `git log --oneline` shows and what the
/// table stores, so a commit is "present" when its full hash starts with the
/// table's short sha. Order follows the table, so the UI is stable.
pub fn known_bad_in(commits: &[String]) -> Vec<&'static KnownBad> {
    hits(KNOWN_BAD, commits)
}

/// Table-generic matcher so the matching rules stay covered while
/// [`KNOWN_BAD`] is empty.
fn hits<'a>(table: &'a [KnownBad], commits: &[String]) -> Vec<&'a KnownBad> {
    table
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

    const SAMPLE: &[KnownBad] = &[KnownBad {
        sha: "2c67e50",
        title: "sample entry",
        issue: 2543,
        advice: "sample advice",
    }];

    #[test]
    fn matches_the_bad_commit_inside_the_incoming_range() {
        let commits = vec![full("1111111"), full("2c67e50")];
        let hits = hits(SAMPLE, &commits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].sha, "2c67e50");
        assert_eq!(hits[0].issue, 2543);
    }

    #[test]
    fn ignores_unrelated_commits() {
        assert!(hits(SAMPLE, &[full("97845dc"), full("9905f0d")]).is_empty());
        assert!(hits(SAMPLE, &[]).is_empty());
        assert!(known_bad_in(&[full("97845dc"), full("9905f0d")]).is_empty());
    }

    #[test]
    fn prefix_must_be_a_prefix_not_a_substring() {
        // A hash that merely contains the short sha is not the commit.
        let mut other = String::from("fffffff");
        other.push_str("2c67e50");
        assert!(hits(SAMPLE, &[other]).is_empty());
        // Shorter than the table entry can never match.
        assert!(hits(SAMPLE, &["2c67e".to_string()]).is_empty());
    }

    #[test]
    fn retired_seedvc_commit_no_longer_blocks() {
        // 86671f15 guards total_size in mmgp/offload.py, so an incoming range
        // that still contains 2c67e50 must not stop the update.
        assert!(known_bad_in(&[full("2c67e50")]).is_empty());
    }

    #[test]
    fn table_entries_are_well_formed() {
        for bad in KNOWN_BAD.iter().chain(SAMPLE) {
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
