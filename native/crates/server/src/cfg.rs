//! FiveM console syntax shared by `server.cfg`, `+command` arguments, rcon
//! and the live console: whitespace-separated words, quotes, `;` separators.

/// Split one line into commands, each a list of words. `#` and `//` start a
/// comment outside quotes.
pub fn parse_line(line: &str) -> Vec<Vec<String>> {
    let mut commands = Vec::new();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut has_word = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => word.push(c),
            None => match c {
                '"' | '\'' => {
                    quote = Some(c);
                    has_word = true;
                }
                '#' if !has_word => break,
                '/' if !has_word && chars.peek() == Some(&'/') => break,
                ';' => {
                    if has_word {
                        words.push(std::mem::take(&mut word));
                        has_word = false;
                    }
                    if !words.is_empty() {
                        commands.push(std::mem::take(&mut words));
                    }
                }
                c if c.is_whitespace() => {
                    if has_word {
                        words.push(std::mem::take(&mut word));
                        has_word = false;
                    }
                }
                c => {
                    word.push(c);
                    has_word = true;
                }
            },
        }
    }
    if has_word {
        words.push(word);
    }
    if !words.is_empty() {
        commands.push(words);
    }
    commands
}

/// FiveM launch arguments: `+exec server.cfg +set sv_maxclients 8`.
pub fn parse_arguments(args: &[String]) -> Vec<Vec<String>> {
    let mut commands: Vec<Vec<String>> = Vec::new();
    for arg in args {
        if let Some(command) = arg.strip_prefix('+') {
            commands.push(vec![command.to_string()]);
        } else if let Some(last) = commands.last_mut() {
            last.push(arg.clone());
        }
    }
    commands.retain(|c| !c[0].is_empty());
    commands
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quotes_comments_and_separators() {
        assert_eq!(
            parse_line(r#"sv_hostname "My  Server" # comment"#),
            vec![vec!["sv_hostname".to_string(), "My  Server".into()]]
        );
        assert!(parse_line("# ensure chat").is_empty());
        assert!(parse_line("   // ensure chat").is_empty());
        assert_eq!(parse_line("ensure chat; ensure spawnmanager").len(), 2);
        assert_eq!(parse_line(r#"set a "x;y""#)[0][2], "x;y");
        assert_eq!(parse_line(r#"set empty """#)[0], vec!["set", "empty", ""]);
        assert_eq!(
            parse_arguments(&[
                "+exec".into(),
                "server.cfg".into(),
                "+set".into(),
                "a".into(),
                "b".into()
            ]),
            vec![
                vec!["exec".to_string(), "server.cfg".into()],
                vec!["set".into(), "a".into(), "b".into()]
            ]
        );
    }
}
