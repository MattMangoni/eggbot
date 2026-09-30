//! Bots hand work to each other by writing `@Name` in a reply.

/// Chains pause after this many automatic handoffs and wait for the user.
pub const MAX_HOPS: u32 = 3;

/// Ids of the bots mentioned as `@Name` (case-insensitive, longest name wins: "@Reviewer 2" is not "@Reviewer").
pub fn mentions(text: &str, bots: &[(usize, &str)], sender: usize) -> Vec<usize> {
    let lower = text.to_lowercase();
    let mut names: Vec<(usize, String)> = bots.iter().map(|(id, n)| (*id, n.to_lowercase())).collect();
    names.sort_by_key(|(_, n)| std::cmp::Reverse(n.len()));
    let mut found = vec![];
    for (at, _) in lower.match_indices('@') {
        let rest = &lower[at + 1..];
        let hit = names.iter().find(|(_, n)| {
            rest.starts_with(n.as_str()) && !rest[n.len()..].starts_with(|c: char| c.is_alphanumeric())
        });
        if let Some((id, _)) = hit
            && *id != sender
            && !found.contains(id)
        {
            found.push(*id);
        }
    }
    found
}

/// Added to every bot's role so it knows who it can hand work to.
pub fn roster(bots: &[(usize, &str, &str)], me: usize) -> String {
    let others: Vec<String> = bots.iter().filter(|(id, ..)| *id != me).map(|(_, name, blurb)| format!("@{name} ({blurb})")).collect();
    if others.is_empty() {
        return String::new();
    }
    format!(
        " Other bots you can hand work to: {}. Writing @Name anywhere in your reply sends your whole reply to that bot, so only write @Name when you want it to act next.",
        others.join(", ")
    )
}

/// What the receiving bot is told.
pub fn prompt(from: &str, text: &str, same_folder: bool) -> String {
    let context = if same_folder {
        "You share the same project folder (/work), so you can open the files they changed."
    } else {
        "They work in a different folder; you cannot see their files."
    };
    format!("Handoff from {from}, another bot in eggbot. {context}\n\n{text}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_mentions() {
        let bots = [(0, "Reviewer"), (1, "Implementer"), (2, "Reviewer 2")];
        assert_eq!(mentions("Done. @reviewer please check, cc @Reviewer 2.", &bots, 1), vec![0, 2]);
        assert_eq!(mentions("@Reviewer2 and @Reviewers and email a@b", &bots, 1), Vec::<usize>::new());
        assert_eq!(mentions("@Implementer thanks, @Reviewer again @Reviewer", &bots, 1), vec![0]);
    }
}
