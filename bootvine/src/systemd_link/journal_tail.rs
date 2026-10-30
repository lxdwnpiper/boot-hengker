use std::{io, str::Chars};

use systemd::{
    id128::Id128,
    journal::{Journal, OpenOptions},
};
use tokio::task::spawn_blocking;

const UNIT_FIELDS: [&str; 4] = ["_SYSTEMD_UNIT", "UNIT", "_SYSTEMD_USER_UNIT", "USER_UNIT"];
const MANAGER_SUMMARY_MESSAGE_IDS: [&str; 10] = [
    "7d4958e842da4a758f6c1cdc7b36dcc5",
    "39f53479d3a045ac8e11786248231fbf",
    "7ad2d189f7e94e70a38c781354912448",
    "5eb03494b6584870a536b337290809b3",
    "be02cf6855d2428ba40df7e9d022f03d",
    "de5b426a63be47a7b6ac3eaac82e2f6f",
    "9d1aaa27d60140bd96365438aad20286",
    "ae8f7b866b0347b9af31fe1c80b127c0",
    "d9b373ed55a64feb8242e02dbe79a49c",
    "98e322203f7a4ed290d09fe03c09fe15",
];
const ENTRIES_SCANNED: usize = 200;
const ESCAPE: char = '\u{1b}';

pub async fn recent_unit_messages(unit: String, wanted: usize) -> Vec<String> {
    spawn_blocking(move || {
        read_recent_messages(&unit, wanted).unwrap_or_else(|error| {
            eprintln!("bootvine: reading the journal of {unit}: {error}");
            Vec::new()
        })
    })
    .await
    .unwrap_or_default()
}

fn read_recent_messages(unit: &str, wanted: usize) -> io::Result<Vec<String>> {
    let mut journal = OpenOptions::default().open()?;
    match_unit_in_this_boot(&mut journal, unit)?;
    journal.seek_tail()?;
    let mut newest_first = Vec::new();
    let mut scanned = 0;
    while newest_first.len() < wanted
        && scanned < ENTRIES_SCANNED
        && journal.previous().is_ok_and(|moved| moved > 0)
    {
        scanned += 1;
        if is_manager_summary(&mut journal).unwrap_or(true) {
            continue;
        }
        if let Ok(Some(message)) = entry_message(&mut journal) {
            newest_first.push(message);
        }
    }
    newest_first.reverse();
    Ok(newest_first)
}

fn match_unit_in_this_boot(journal: &mut Journal, unit: &str) -> io::Result<()> {
    UNIT_FIELDS.iter().try_for_each(|field| {
        journal.match_add(field, unit)?.match_or()?;
        Ok::<_, io::Error>(())
    })?;
    journal
        .match_and()?
        .match_add("_BOOT_ID", Id128::from_boot()?.to_string())?;
    Ok(())
}

fn field_text(journal: &mut Journal, field: &str) -> io::Result<Option<String>> {
    Ok(journal.get_data(field)?.and_then(|entry_field| {
        entry_field
            .value()
            .map(|value| String::from_utf8_lossy(value).into_owned())
    }))
}

fn is_manager_summary(journal: &mut Journal) -> io::Result<bool> {
    Ok(field_text(journal, "MESSAGE_ID")?
        .is_some_and(|message_id| MANAGER_SUMMARY_MESSAGE_IDS.contains(&message_id.as_str())))
}

fn entry_message(journal: &mut Journal) -> io::Result<Option<String>> {
    Ok(field_text(journal, "MESSAGE")?
        .map(|message| printable_line(&message))
        .filter(|line| !line.is_empty()))
}

fn printable_line(message: &str) -> String {
    let mut characters = message.lines().next().unwrap_or_default().chars();
    let mut printable = String::new();
    while let Some(character) = characters.next() {
        if character == ESCAPE {
            skip_escape_sequence(&mut characters);
        } else if !character.is_control() {
            printable.push(character);
        }
    }
    printable.trim_end().to_owned()
}

fn skip_escape_sequence(characters: &mut Chars<'_>) {
    if characters.clone().next() == Some('[') {
        characters.next();
        characters.find(|character| ('@'..='~').contains(character));
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    #[test]
    #[ignore = "reads this machine's journal: cargo test journal_of_this_boot -- --ignored \
                --nocapture"]
    fn journal_of_this_boot_has_lines_for_journald() {
        let unit = env::var("BOOTVINE_JOURNAL_UNIT")
            .unwrap_or_else(|_| "systemd-journald.service".to_owned());
        let lines = read_recent_messages(&unit, 6).expect("readable journal");
        lines.iter().for_each(|line| println!("{line}"));
        assert!(!lines.is_empty());
    }

    #[test]
    fn printable_line_keeps_the_first_line_without_colours() {
        assert_eq!(
            printable_line("\u{1b}[31mno such file\u{1b}[0m\nsecond line"),
            "no such file"
        );
    }
}
