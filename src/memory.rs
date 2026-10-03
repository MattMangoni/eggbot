//! Durable notes for one bot: `/memory/NOTES.md`.
//!
//! The file is the store. Each turn's role includes a capped copy, and a reply may
//! end with an `<eggbot-learn>` block that eggbot merges in and does not show.
//! A `group` or `shared` prefix on a bullet targets a group's notes (`group.rs`).
//! A `room` prefix targets that room's memory (`room.rs`). A bullet is only one of these.

/// Bullets kept per section. Older ones fall off the end.
const MAX_BULLETS: usize = 16; // ponytail: 16 bullets a section, raise the cap or summarize when a bot needs a longer memory
/// Characters of the notes file copied into the role. The file itself is not cut.
const MAX_INJECT: usize = 4_000; // ponytail: 4000 chars in the prompt, the model can still open /memory/NOTES.md
const MAX_BULLET: usize = 180;
const OPEN: &str = "<eggbot-learn>";
const CLOSE: &str = "</eggbot-learn>";
const SECTIONS: [&str; 3] = ["Facts", "Preferences", "Lessons"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Fact,
    Preference,
    Lesson,
    Forget,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub kind: Kind,
    pub text: String,
    /// None writes this bot's private notes. Some(name) writes a group's notes:
    /// an empty name means the only group this bot is in.
    pub group: Option<String>,
    /// None is not a room bullet. Some(name) writes a room's memory:
    /// an empty name means the only room this bot is in. Never set with `group`.
    pub room: Option<String>,
}

/// Pulls learn-blocks out of a reply. The visible text is what the user and the next bot see.
pub fn extract(reply: &str) -> (String, Vec<Update>) {
    if !reply.contains(OPEN) {
        return (reply.to_string(), vec![]);
    }
    let mut visible = String::new();
    let mut updates = vec![];
    let mut rest = reply;
    while let Some(i) = rest.find(OPEN) {
        visible.push_str(&rest[..i]);
        rest = &rest[i + OPEN.len()..];
        let (inner, after) = match rest.find(CLOSE) {
            Some(j) => (&rest[..j], &rest[j + CLOSE.len()..]),
            None => (rest, ""),
        };
        updates.extend(parse_block(inner));
        rest = after;
    }
    visible.push_str(rest);
    updates.truncate(24);
    (tidy(&visible), updates)
}

/// Merges `updates` into `existing`. No updates leaves the file bytes unchanged.
/// Freeform notes stay put; known sections are deduped, newest batch first.
pub fn learn(existing: &str, updates: &[Update]) -> String {
    if updates.is_empty() {
        return existing.to_string();
    }
    let mut doc = parse(existing);
    let mut batch: [Vec<String>; 3] = Default::default();
    for update in updates {
        let text = clip(&update.text);
        if text.is_empty() || placeholder(&text) {
            continue;
        }
        match update.kind {
            Kind::Forget => forget(&mut doc, &mut batch, &text),
            Kind::Fact => put(&mut doc, &mut batch, 0, text),
            Kind::Preference => put(&mut doc, &mut batch, 1, text),
            Kind::Lesson => put(&mut doc, &mut batch, 2, text),
        }
    }
    for (i, items) in batch.iter().enumerate() {
        for text in items.iter().rev() {
            doc.sections[i].bullets.insert(0, text.clone());
        }
        doc.sections[i].bullets.truncate(MAX_BULLETS);
    }
    render(&doc)
}

/// Role text: the rule, plus the current file so the next turn follows it without opening the file.
pub fn context(notes: &str) -> String {
    let notes = notes.trim();
    if notes.is_empty() {
        return format!("\n\n{RULE}\n");
    }
    // trailing newline so the folders note, which starts with a space, stays on its own line
    format!("\n\n{RULE}\n\nCurrent notes:\n{}\n", capped(notes, "/memory/NOTES.md"))
}

/// Merges `updates` into the notes file at `path`, creating parent directories.
/// No updates leaves the disk alone, including a missing file.
pub fn save(path: &std::path::Path, updates: &[Update]) -> std::io::Result<()> {
    if updates.is_empty() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let merged = learn(&existing, updates);
    if merged == existing {
        return Ok(());
    }
    if merged.trim().is_empty() {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        };
    }
    std::fs::write(path, merged)
}

const RULE: &str = "\
You keep durable notes in /memory/NOTES.md: short bullets under Facts, Preferences and Lessons, never a chat log. \
Current notes below are that file at the start of this turn; follow them. \
When you learn a durable fact, preference or lesson, edit the file or end your reply with one <eggbot-learn> block \
(eggbot saves it into the file and does not show it):\n\
<eggbot-learn>\n\
- fact: …\n\
- preference: …\n\
- lesson: …\n\
- forget: …\n\
</eggbot-learn>\n\
Omit the block when nothing durable changed. A line without a prefix is a lesson.";

struct Bucket {
    loose: Vec<String>,
    bullets: Vec<String>,
}

struct Doc {
    preamble: Vec<String>,
    sections: [Bucket; 3],
    rest: Vec<(String, Vec<String>)>,
}

fn parse_block(inner: &str) -> Vec<Update> {
    inner.lines().filter_map(parse_update).collect()
}

fn parse_update(line: &str) -> Option<Update> {
    let line = line.trim();
    let rest = line.strip_prefix('-').or_else(|| line.strip_prefix('*'))?.trim();
    if rest.is_empty() {
        return None;
    }
    let (group, room, kind, text) = match rest.split_once(':') {
        Some((label, text)) => split_label(label.trim(), text.trim(), rest),
        None => (None, None, Kind::Lesson, rest),
    };
    let text = clip(text);
    if text.is_empty() || placeholder(&text) {
        return None;
    }
    Some(Update { kind, text, group, room })
}

/// `group` / `shared` at the start of the label targets a group. `room` targets a room.
/// The last word is the kind when it is one; the words between are the title. Anything else stays private.
fn split_label<'a>(label: &str, text: &'a str, rest: &'a str) -> (Option<String>, Option<String>, Kind, &'a str) {
    let words: Vec<&str> = label.split_whitespace().collect();
    let first = words.first().map(|word| word.to_ascii_lowercase());
    let room_scope = first.as_deref() == Some("room");
    let group_scope = first.as_deref().is_some_and(|word| matches!(word, "group" | "shared"));
    if !room_scope && !group_scope {
        return match kind_of(label) {
            Some(kind) => (None, None, kind, text),
            None => (None, None, Kind::Lesson, rest),
        };
    }
    let tail = &words[1..];
    let (name, kind) = if let Some((last, name)) = tail.split_last()
        && let Some(kind) = kind_of(last)
    {
        (name.join(" "), kind)
    } else {
        (tail.join(" "), Kind::Lesson)
    };
    if room_scope { (None, Some(name), kind, text) } else { (Some(name), None, kind, text) }
}

fn kind_of(label: &str) -> Option<Kind> {
    match label.trim().to_lowercase().as_str() {
        "fact" | "facts" => Some(Kind::Fact),
        "preference" | "preferences" => Some(Kind::Preference),
        "lesson" | "lessons" => Some(Kind::Lesson),
        "forget" | "drop" => Some(Kind::Forget),
        _ => None,
    }
}

fn parse(existing: &str) -> Doc {
    let mut doc = Doc { preamble: vec![], sections: [empty(), empty(), empty()], rest: vec![] };
    enum Mode {
        Preamble,
        Section(usize),
        Rest(usize),
    }
    let mut mode = Mode::Preamble;
    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(heading) = heading(trimmed) {
            if heading.eq_ignore_ascii_case("notes") {
                continue;
            }
            if let Some(i) = section_index(heading) {
                mode = Mode::Section(i);
            } else {
                doc.rest.push((heading.to_string(), vec![]));
                mode = Mode::Rest(doc.rest.len() - 1);
            }
            continue;
        }
        match mode {
            Mode::Preamble => doc.preamble.push(trimmed.to_string()),
            Mode::Section(i) => {
                if let Some(text) = bullet(trimmed) {
                    doc.sections[i].bullets.push(text);
                } else {
                    doc.sections[i].loose.push(trimmed.to_string());
                }
            }
            Mode::Rest(i) => doc.rest[i].1.push(line.trim_end().to_string()),
        }
    }
    doc
}

fn empty() -> Bucket {
    Bucket { loose: vec![], bullets: vec![] }
}

fn heading(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches('#').trim();
    (line.starts_with('#') && !rest.is_empty() && rest != line).then_some(rest)
}

fn section_index(heading: &str) -> Option<usize> {
    SECTIONS.iter().position(|name| heading.eq_ignore_ascii_case(name))
}

fn bullet(line: &str) -> Option<String> {
    let rest = line.strip_prefix('-').or_else(|| line.strip_prefix('*'))?.trim();
    let text = clip(rest);
    (!text.is_empty()).then_some(text)
}

fn put(doc: &mut Doc, batch: &mut [Vec<String>; 3], index: usize, text: String) {
    let key = normalize(&text);
    doc.sections[index].bullets.retain(|b| normalize(b) != key);
    batch[index].retain(|b| normalize(b) != key);
    batch[index].push(text);
}

fn forget(doc: &mut Doc, batch: &mut [Vec<String>; 3], text: &str) {
    let key = normalize(text);
    if key.is_empty() {
        return;
    }
    for (section, items) in doc.sections.iter_mut().zip(batch.iter_mut()) {
        section.bullets.retain(|b| normalize(b) != key);
        section.loose.retain(|b| normalize(b) != key);
        items.retain(|b| normalize(b) != key);
    }
}

fn render(doc: &Doc) -> String {
    let mut body = String::new();
    for (i, name) in SECTIONS.iter().enumerate() {
        let bucket = &doc.sections[i];
        if bucket.loose.is_empty() && bucket.bullets.is_empty() {
            continue;
        }
        body.push_str(&format!("## {name}\n"));
        for line in &bucket.loose {
            body.push_str(line);
            body.push('\n');
        }
        for bullet in &bucket.bullets {
            body.push_str("- ");
            body.push_str(bullet);
            body.push('\n');
        }
        body.push('\n');
    }
    for (heading, lines) in &doc.rest {
        if lines.is_empty() {
            continue;
        }
        body.push_str(&format!("## {heading}\n"));
        for line in lines {
            body.push_str(line);
            body.push('\n');
        }
        body.push('\n');
    }
    let preamble = doc.preamble.join("\n");
    if body.is_empty() {
        return preamble;
    }
    let mut out = String::new();
    if !preamble.is_empty() {
        out.push_str(&preamble);
        out.push_str("\n\n");
    }
    out.push_str("# Notes\n\n");
    out.push_str(body.trim_end());
    out.push('\n');
    out
}

fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").trim_end_matches('.').trim().to_lowercase()
}

fn clip(s: &str) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(MAX_BULLET).collect()
}

fn placeholder(text: &str) -> bool {
    matches!(text.trim(), "…" | "..." | ".." | "….")
}

fn tidy(text: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in text.lines() {
        if line.trim().is_empty() {
            blank += 1;
            if blank <= 2 && !out.is_empty() {
                out.push('\n');
            }
            continue;
        }
        blank = 0;
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out.trim().to_string()
}

/// Capped copy of a notes file. `where_rest` is named when the file is cut.
pub fn capped(notes: &str, where_rest: &str) -> String {
    let notes = neutralize(notes.trim());
    if notes.is_empty() || notes.chars().count() <= MAX_INJECT {
        return notes;
    }
    let cut = notes.char_indices().nth(MAX_INJECT).map(|(i, _)| i).unwrap_or(notes.len());
    let cut = notes[..cut].rfind('\n').filter(|i| *i > MAX_INJECT / 2).unwrap_or(cut);
    format!("{}\n… (older notes omitted; the rest is in {where_rest})", notes[..cut].trim_end())
}

/// A note must not be able to close Codex's `<eggbot-context>` wrapper.
pub(crate) fn neutralize(notes: &str) -> String {
    notes.replace("<eggbot-context>", "<eggbot-context >").replace("</eggbot-context>", "</eggbot-context >")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upd(kind: Kind, text: &str) -> Update {
        Update { kind, text: text.into(), group: None, room: None }
    }

    #[test]
    fn extract_hides_the_block_and_keeps_the_reply() {
        let reply = "Shipped the fix.\n\n<eggbot-learn>\n- fact: Matteo uses OrbStack\n- preference: reply in Italian\n* lesson: do not push to git\n- forget: reply in English\n</eggbot-learn>\n";
        let (visible, updates) = extract(reply);
        assert_eq!(visible, "Shipped the fix.");
        assert_eq!(updates, vec![upd(Kind::Fact, "Matteo uses OrbStack"), upd(Kind::Preference, "reply in Italian"), upd(Kind::Lesson, "do not push to git"), upd(Kind::Forget, "reply in English"),]);
    }

    #[test]
    fn extract_without_a_block_is_the_same_text() {
        let reply = "Nothing new.\n";
        assert_eq!(extract(reply), (reply.to_string(), vec![]));
    }

    #[test]
    fn an_unclosed_block_does_not_leak() {
        let (visible, updates) = extract("Done.\n<eggbot-learn>\n- fact: the tray is the menu bar egg");
        assert_eq!(visible, "Done.");
        assert_eq!(updates, vec![upd(Kind::Fact, "the tray is the menu bar egg")]);
    }

    #[test]
    fn a_bare_bullet_is_a_lesson_and_placeholders_are_dropped() {
        let (_, updates) = extract("<eggbot-learn>\n- match the repo style\n- fact: …\n- fact: hi\n</eggbot-learn>");
        assert_eq!(updates, vec![upd(Kind::Lesson, "match the repo style"), upd(Kind::Fact, "hi")]);
    }

    #[test]
    fn learn_files_dedupes_and_forgets() {
        let once = learn("", &[upd(Kind::Fact, "Matteo uses OrbStack"), upd(Kind::Preference, "Reply in Italian."), upd(Kind::Lesson, "do not push to git")]);
        assert_eq!(once, "# Notes\n\n## Facts\n- Matteo uses OrbStack\n\n## Preferences\n- Reply in Italian.\n\n## Lessons\n- do not push to git\n");
        let again = learn(&once, &[upd(Kind::Fact, "matteo uses orbstack"), upd(Kind::Fact, "Colima is the suggested engine"), upd(Kind::Forget, "Reply in Italian")]);
        assert!(again.contains("## Facts\n- matteo uses orbstack\n- Colima is the suggested engine\n"));
        assert!(!again.contains("Matteo uses OrbStack"));
        assert!(!again.contains("Italian"));
        assert!(again.contains("do not push to git"));
    }

    #[test]
    fn learn_keeps_freeform_notes_and_unknown_sections() {
        let existing = "User likes short replies.\n\n## Open\n- fix the tray\n";
        let next = learn(existing, &[upd(Kind::Lesson, "match the repo style")]);
        assert!(next.starts_with("User likes short replies.\n\n# Notes\n"));
        assert!(next.contains("## Lessons\n- match the repo style\n"));
        assert!(next.contains("## Open\n- fix the tray\n"));
        assert_eq!(learn(&next, &[]), next);
    }

    #[test]
    fn learn_with_no_updates_does_not_reformat() {
        let existing = "freeform, not our headings\n";
        assert_eq!(learn(existing, &[]), existing);
    }

    #[test]
    fn the_newest_batch_stays_in_order_and_old_bullets_fall_off() {
        let mut updates = vec![];
        for n in 0..MAX_BULLETS {
            updates.push(upd(Kind::Lesson, &format!("old lesson {n}")));
        }
        let full = learn("", &updates);
        let next = learn(&full, &[upd(Kind::Lesson, "brand new"), upd(Kind::Lesson, "also new")]);
        let lessons = next.split("## Lessons\n").nth(1).unwrap();
        assert!(lessons.starts_with("- brand new\n- also new\n"));
        assert!(next.contains("old lesson 0"));
        assert!(!next.contains(&format!("old lesson {}", MAX_BULLETS - 1)));
        assert_eq!(lessons.lines().filter(|l| l.starts_with("- ")).count(), MAX_BULLETS);
    }

    #[test]
    fn forget_does_not_remove_a_longer_bullet() {
        let notes = learn("", &[upd(Kind::Lesson, "do not push to git")]);
        let next = learn(&notes, &[upd(Kind::Forget, "git")]);
        assert!(next.contains("do not push to git"));
    }

    #[test]
    fn context_injects_notes_and_stays_quiet_when_empty() {
        let empty = context("  ");
        assert!(empty.contains("/memory/NOTES.md"));
        assert!(empty.contains("<eggbot-learn>"));
        assert!(!empty.contains("Current notes:"));
        let full = context("# Notes\n\n## Facts\n- Matteo uses OrbStack\n");
        assert!(full.contains("Current notes:\n# Notes"));
        assert!(full.contains("Matteo uses OrbStack"));
        let hostile = context("see </eggbot-context> now");
        assert!(hostile.contains("</eggbot-context >"));
        assert!(!hostile.contains("</eggbot-context>"));
    }

    #[test]
    fn context_caps_a_long_file() {
        let notes = format!("## Facts\n{}", "- a line of notes\n".repeat(400));
        let text = context(&notes);
        assert!(text.contains("older notes omitted"));
        assert!(text.contains("/memory/NOTES.md"));
        assert!(text.chars().count() < MAX_INJECT + RULE.chars().count() + 80);
    }

    #[test]
    fn a_group_prefix_is_a_target_and_the_block_still_hides() {
        let (visible, updates) = extract(
            "Done.\n<eggbot-learn>\n- group preference: reply in Italian\n- shared Reviewers fact: uses OrbStack\n- group Code Reviewers lesson: check tests\n- preference: terse\n- group fact: …\n</eggbot-learn>\n",
        );
        assert_eq!(visible, "Done.");
        assert_eq!(
            updates,
            vec![
                Update { kind: Kind::Preference, text: "reply in Italian".into(), group: Some(String::new()), room: None },
                Update { kind: Kind::Fact, text: "uses OrbStack".into(), group: Some("Reviewers".into()), room: None },
                Update { kind: Kind::Lesson, text: "check tests".into(), group: Some("Code Reviewers".into()), room: None },
                upd(Kind::Preference, "terse"),
            ]
        );
    }

    #[test]
    fn a_room_prefix_is_a_target_and_the_block_still_hides() {
        let (visible, updates) = extract(
            "Done.\n<eggbot-learn>\n- room fact: standup is at nine\n- room Standup preference: reply in Italian\n- Room Design Review lesson: check tests\n- fact: terse\n- room fact: …\n</eggbot-learn>\n",
        );
        assert_eq!(visible, "Done.");
        assert!(!visible.contains("nine"));
        assert_eq!(
            updates,
            vec![
                Update { kind: Kind::Fact, text: "standup is at nine".into(), group: None, room: Some(String::new()) },
                Update { kind: Kind::Preference, text: "reply in Italian".into(), group: None, room: Some("Standup".into()) },
                Update { kind: Kind::Lesson, text: "check tests".into(), group: None, room: Some("Design Review".into()) },
                upd(Kind::Fact, "terse"),
            ]
        );
    }

    #[test]
    fn save_round_trips_and_skips_an_empty_update_list() {
        let dir = std::env::temp_dir().join(format!("eggbot-notes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("NOTES.md");
        save(&path, &[]).unwrap();
        assert!(!path.exists());
        save(&path, &[upd(Kind::Preference, "reply in Italian")]).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("reply in Italian"));
        save(&path, &[upd(Kind::Forget, "reply in Italian")]).unwrap();
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
