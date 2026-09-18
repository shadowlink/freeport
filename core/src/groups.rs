//! Grouping of catalog projects by game.
//!
//! Several projects can port the *same* game (Zelda64Recomp and 2Ship are both
//! Majora's Mask; Perfect Dark PC and Dab's Mod; …). The catalog marks them with
//! a shared `game_id`; this module folds them into one [`GameGroup`] per game so
//! the UI can show a single cover with a version selector.
//!
//! Grouping is pure and order-preserving: groups appear in the order their first
//! member appears in the input, and members keep the input order too. Which
//! member is the *primary* (shown by default) is decided by [`pick_primary`].

use crate::model::Project;

/// One card in the catalog: a game and every project that ports it.
#[derive(Debug, Clone)]
pub struct GameGroup<'a> {
    /// `Project::game_key()` shared by all members.
    pub key: String,
    /// Index into `members` of the version shown by default.
    pub primary: usize,
    pub members: Vec<&'a Project>,
}

impl<'a> GameGroup<'a> {
    pub fn primary(&self) -> &'a Project {
        self.members[self.primary]
    }

    /// True when the card has a version selector to show.
    pub fn has_versions(&self) -> bool {
        self.members.len() > 1
    }

    /// Display title: the real game's name (falls back to the project name).
    pub fn title(&self) -> &str {
        let p = self.primary();
        if p.original_game.is_empty() {
            &p.name
        } else {
            &p.original_game
        }
    }
}

/// Fold `projects` into one group per `game_key()`, preserving first-seen order.
/// `native_triple` is the running platform (e.g. `linux-x86_64`), used to pick
/// each group's primary version.
pub fn group_projects<'a, I>(projects: I, native_triple: &str) -> Vec<GameGroup<'a>>
where
    I: IntoIterator<Item = &'a Project>,
{
    let mut groups: Vec<GameGroup<'a>> = Vec::new();
    for p in projects {
        let key = p.game_key();
        match groups.iter_mut().find(|g| g.key == key) {
            Some(g) => g.members.push(p),
            None => groups.push(GameGroup { key: key.to_string(), primary: 0, members: vec![p] }),
        }
    }
    for g in &mut groups {
        g.primary = pick_primary(&g.members, native_triple);
    }
    groups
}

/// Choose the version shown by default for a group. In order of priority:
/// 1. a member the catalog marks `preferred`,
/// 2. a member with a native build for `native_triple`,
/// 3. the most recently published release (`cached.published_at`, ISO-8601 so
///    lexical order is chronological),
/// 4. the first member.
/// Every tie is broken by input order, so the result is stable.
pub fn pick_primary(members: &[&Project], native_triple: &str) -> usize {
    if members.len() <= 1 {
        return 0;
    }
    if let Some(i) = members.iter().position(|p| p.preferred) {
        return i;
    }
    let native: Vec<usize> =
        (0..members.len()).filter(|&i| members[i].supports(native_triple)).collect();
    let pool: Vec<usize> = if native.is_empty() { (0..members.len()).collect() } else { native };
    let published = |i: usize| members[i].cached.as_ref().and_then(|c| c.published_at.as_deref()).unwrap_or("");
    let mut best = pool[0];
    for &i in &pool[1..] {
        if published(i) > published(best) {
            best = i;
        }
    }
    best
}

/// Group only the projects that pass `visible`, so hidden versions (e.g. a
/// Windows-only build with Windows mode off) neither create cards nor appear in
/// the version selector.
pub fn group_visible<'a, I, F>(projects: I, native_triple: &str, mut visible: F) -> Vec<GameGroup<'a>>
where
    I: IntoIterator<Item = &'a Project>,
    F: FnMut(&Project) -> bool,
{
    group_projects(projects.into_iter().filter(|p| visible(p)), native_triple)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Cached, Project, RepoRef, RomInfo};
    use std::collections::HashMap;

    fn proj(id: &str, game: &str, game_id: Option<&str>, platforms: &[&str], published: &str) -> Project {
        Project {
            id: id.into(),
            name: format!("{id} port"),
            original_game: game.into(),
            game_id: game_id.map(Into::into),
            preferred: false,
            system: "n64".into(),
            kind: "native-port".into(),
            repo: RepoRef { host: "github".into(), owner: "o".into(), repo: id.into() },
            release_channel: "stable".into(),
            rolling_tag: None,
            cover_url: None,
            box_art: None,
            logo_url: None,
            year: None,
            developer: None,
            genre: None,
            wiki: None,
            mods: None,
            ra_supported: false,
            ra_beta: false,
            launch_env: HashMap::new(),
            direct: None,
            asset_rules: platforms.iter().map(|t| ((*t).to_string(), ".*".to_string())).collect(),
            rom: RomInfo::default(),
            launch: Default::default(),
            cached: Some(Cached {
                platforms: platforms.iter().map(|s| s.to_string()).collect(),
                latest_tag: Some("v1".into()),
                published_at: if published.is_empty() { None } else { Some(published.into()) },
            }),
        }
    }

    const LIN: &str = "linux-x86_64";
    const WIN: &str = "windows-x86_64";

    #[test]
    fn game_key_falls_back_to_id() {
        let a = proj("a", "A", None, &[LIN], "");
        assert_eq!(a.game_key(), "a");
        let b = proj("b", "B", Some("shared"), &[LIN], "");
        assert_eq!(b.game_key(), "shared");
        let c = proj("c", "C", Some(""), &[LIN], "");
        assert_eq!(c.game_key(), "c");
    }

    #[test]
    fn groups_by_game_id_preserving_order() {
        let ps = vec![
            proj("mm-recomp", "Majora's Mask", Some("mm"), &[LIN, WIN], "2026-01-01"),
            proj("soh", "Ocarina", Some("oot"), &[LIN], "2026-01-01"),
            proj("2ship", "Majora's Mask", Some("mm"), &[LIN, WIN], "2026-02-01"),
            proj("solo", "Solo", None, &[LIN], ""),
        ];
        let g = group_projects(&ps, LIN);
        assert_eq!(g.iter().map(|g| g.key.as_str()).collect::<Vec<_>>(), ["mm", "oot", "solo"]);
        assert_eq!(g[0].members.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["mm-recomp", "2ship"]);
        assert!(g[0].has_versions());
        assert!(!g[1].has_versions());
        assert_eq!(g[0].title(), "Majora's Mask");
    }

    #[test]
    fn primary_prefers_flag_then_native_then_newest() {
        // Newest wins among natives.
        let a = proj("old", "G", Some("g"), &[LIN], "2025-01-01");
        let b = proj("new", "G", Some("g"), &[LIN], "2026-01-01");
        assert_eq!(pick_primary(&[&a, &b], LIN), 1);
        // A native build beats a newer Windows-only one.
        let w = proj("win-new", "G", Some("g"), &[WIN], "2026-06-01");
        assert_eq!(pick_primary(&[&w, &a], LIN), 1);
        // Nothing native: newest overall.
        let w2 = proj("win-old", "G", Some("g"), &[WIN], "2024-01-01");
        assert_eq!(pick_primary(&[&w2, &w], LIN), 1);
        // `preferred` overrides everything.
        let mut pref = proj("pref", "G", Some("g"), &[WIN], "2020-01-01");
        pref.preferred = true;
        assert_eq!(pick_primary(&[&a, &b, &pref], LIN), 2);
        // Ties keep input order.
        let c = proj("c", "G", Some("g"), &[LIN], "2026-01-01");
        assert_eq!(pick_primary(&[&b, &c], LIN), 0);
    }

    #[test]
    fn group_visible_drops_hidden_members() {
        let ps = vec![
            proj("lin", "G", Some("g"), &[LIN], "2026-01-01"),
            proj("win", "G", Some("g"), &[WIN], "2026-05-01"),
            proj("win-only-game", "H", Some("h"), &[WIN], "2026-05-01"),
        ];
        let g = group_visible(&ps, LIN, |p| p.supports(LIN));
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].members.len(), 1);
        assert_eq!(g[0].primary().id, "lin");
    }
}
