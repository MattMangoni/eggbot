//! Procedures a bot always has. A preset copies its defaults onto the bot at hatch;
//! after that the bot owns the list. They ride in the role text, same channel as notes.

use serde::{Deserialize, Serialize};

/// One instruction pack. `name` is the heading the bot sees; `body` is the procedure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub body: String,
}

pub const MAX_SKILLS: usize = 12;
pub const MAX_NAME: usize = 48;
pub const MAX_BODY: usize = 4000;

/// Starting skills for a preset, matched by name so reordering presets cannot mix them.
/// Custom, and any other name, starts empty.
pub fn defaults(preset: &str) -> Vec<Skill> {
    match preset {
        "Reviewer" => vec![
            skill(
                "Review a diff",
                "Read the diff and the code around each change before you comment. Report only issues you can point at. For each one give the file, the line, why it matters, and a concrete fix. Rank them blocker, should-fix, then nit. Do not edit files unless asked.",
            ),
            skill(
                "Security pass",
                "When a change touches auth, input, files, network, or secrets, check for injection, missing checks, leaked credentials, and unsafe defaults. Say there are no security issues only after that pass.",
            ),
        ],
        "Implementer" => vec![
            skill(
                "Smallest change",
                "Change only what the task needs. Match the names, style, and patterns already in the file. Reuse a helper that exists instead of adding a new one. Say what you changed and what you left alone.",
            ),
            skill(
                "Check the work",
                "Before you call the task done, run the check this project already uses (its tests or its build) when you can. If you cannot run it, say exactly what you did not run.",
            ),
        ],
        "Designer" => vec![
            skill(
                "Critique the screen",
                "Judge hierarchy, spacing, type, color, and alignment against what is already on the screen. For each point name a concrete change: what to move, resize, or restyle.",
            ),
            skill(
                "States and access",
                "Cover empty, loading, error, and success, and keyboard focus and contrast. Call out anything a keyboard or screen-reader user cannot do.",
            ),
        ],
        _ => vec![],
    }
}

/// Adds a skill, or replaces `replacing`. The list is unchanged when this returns an error.
pub fn upsert(skills: &mut Vec<Skill>, replacing: Option<usize>, name: &str, body: &str) -> Result<(), &'static str> {
    if let Some(i) = replacing {
        if i >= skills.len() {
            return Err("That skill is gone");
        }
    } else if skills.len() >= MAX_SKILLS {
        return Err("A bot can have at most 12 skills");
    }
    let skill = normalize(name, body, skills, replacing)?;
    match replacing {
        Some(i) => skills[i] = skill,
        None => skills.push(skill),
    }
    Ok(())
}

pub fn remove(skills: &mut Vec<Skill>, index: usize) -> bool {
    if index < skills.len() {
        skills.remove(index);
        true
    } else {
        false
    }
}

/// Text inserted after the bot's role. Empty when there are no skills, so the role string stays as it was.
pub fn section(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "\n\nSkills you always have. Follow one when the task matches it. A direct instruction from the user wins over a skill.\n",
    );
    for skill in skills {
        out.push_str("\n## ");
        out.push_str(&skill.name);
        out.push('\n');
        out.push_str(&skill.body);
        out.push('\n');
    }
    out
}

/// The role string for one turn. With no skills this is `{role}{others}{notes}{folders}\n\n{shared}`.
pub fn role_text(role: &str, skills: &[Skill], others: &str, notes: &str, folders: &str, shared: &str) -> String {
    format!("{role}{}{others}{notes}{folders}\n\n{shared}", section(skills))
}

fn skill(name: &str, body: &str) -> Skill {
    Skill { name: name.to_string(), body: body.to_string() }
}

fn normalize(name: &str, body: &str, existing: &[Skill], replacing: Option<usize>) -> Result<Skill, &'static str> {
    if name.chars().any(|c| c == '\n' || c == '\r') {
        return Err("Keep the skill name on one line");
    }
    let name = name.trim();
    let body = body.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err("Use a skill name of 1 to 48 characters");
    }
    if body.is_empty() {
        return Err("Write what the skill should do");
    }
    if body.chars().count() > MAX_BODY {
        return Err("Keep a skill under 4000 characters");
    }
    let clash = existing.iter().enumerate().any(|(i, s)| Some(i) != replacing && s.name.eq_ignore_ascii_case(name));
    if clash {
        return Err("This bot already has a skill with that name");
    }
    Ok(skill(name, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_ship_their_own_skills_and_custom_starts_empty() {
        let reviewer = defaults("Reviewer");
        let implementer = defaults("Implementer");
        let designer = defaults("Designer");
        assert_eq!(reviewer.len(), 2);
        assert_eq!(reviewer[0].name, "Review a diff");
        assert!(reviewer[1].name == "Security pass" && reviewer[1].body.contains("injection"));
        assert_eq!(implementer[0].name, "Smallest change");
        assert!(implementer[1].body.contains("did not run"));
        assert_eq!(designer[0].name, "Critique the screen");
        assert!(designer[1].body.contains("screen-reader"));
        assert!(defaults("Custom").is_empty());
        assert!(defaults("custom").is_empty());
        assert!(defaults("").is_empty());
        assert_ne!(reviewer, implementer);
    }

    #[test]
    fn each_hatch_gets_its_own_copy() {
        let mut a = defaults("Reviewer");
        let b = defaults("Reviewer");
        a[0].body.push_str(" extra");
        assert_ne!(a[0].body, b[0].body);
    }

    #[test]
    fn empty_skills_leave_the_role_string_unchanged() {
        let role = "You are Reviewer.";
        let others = " Other bots you can hand work to: @Implementer (writes code).";
        let notes = " You keep notes.";
        let got = role_text(role, &[], others, notes, "", "Reply in markdown.");
        assert_eq!(got, format!("{role}{others}{notes}\n\nReply in markdown."));
        assert_eq!(section(&[]), "");
    }

    #[test]
    fn skills_sit_after_the_role_and_before_the_roster() {
        let mut skills = vec![];
        upsert(&mut skills, None, "  Check  ", "  Run tests.\n").unwrap();
        assert_eq!(skills[0].name, "Check");
        assert_eq!(skills[0].body, "Run tests.");
        let got = role_text("ROLE", &skills, " OTHERS", " NOTES", " FOLDERS", "SHARED");
        assert!(got.starts_with("ROLE\n\nSkills you always have."));
        assert!(got.contains("A direct instruction from the user wins over a skill."));
        assert!(got.contains("## Check\nRun tests.\n OTHERS NOTES FOLDERS\n\nSHARED"));
    }

    #[test]
    fn upsert_rejects_blanks_duplicates_and_the_cap() {
        let mut skills = vec![];
        assert_eq!(upsert(&mut skills, None, "  ", "body"), Err("Use a skill name of 1 to 48 characters"));
        assert_eq!(upsert(&mut skills, None, "Name\n", "body"), Err("Keep the skill name on one line"));
        assert_eq!(upsert(&mut skills, None, "Name", "  "), Err("Write what the skill should do"));
        assert!(upsert(&mut skills, None, "Name", "Do the thing.").is_ok());
        assert_eq!(upsert(&mut skills, None, "name", "Again."), Err("This bot already has a skill with that name"));
        assert!(skills.len() == 1 && skills[0].body == "Do the thing.");
        assert!(upsert(&mut skills, Some(0), "name", "Updated.").is_ok());
        assert_eq!(skills[0].body, "Updated.");
        assert_eq!(upsert(&mut skills, Some(3), "Other", "x"), Err("That skill is gone"));
        assert_eq!(skills.len(), 1);

        let long_name = "n".repeat(MAX_NAME + 1);
        assert!(upsert(&mut skills, None, &long_name, "body").is_err());
        let long_body = "b".repeat(MAX_BODY + 1);
        assert_eq!(upsert(&mut skills, None, "Long", &long_body), Err("Keep a skill under 4000 characters"));

        while skills.len() < MAX_SKILLS {
            let name = format!("S{}", skills.len());
            upsert(&mut skills, None, &name, "body").unwrap();
        }
        assert_eq!(upsert(&mut skills, None, "One more", "body"), Err("A bot can have at most 12 skills"));
        assert!(upsert(&mut skills, Some(0), "Renamed", "still here").is_ok());
        assert_eq!(skills.len(), MAX_SKILLS);
        assert!(remove(&mut skills, 0));
        assert_eq!(skills[0].name, "S1");
        assert!(!remove(&mut skills, 99));
    }

    #[test]
    fn a_skill_round_trips_through_json() {
        let skill = skill("Check", "Run tests.");
        let json = serde_json::to_string(&skill).unwrap();
        assert_eq!(serde_json::from_str::<Skill>(&json).unwrap(), skill);
    }
}
