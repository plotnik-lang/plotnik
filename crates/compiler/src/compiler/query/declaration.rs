/// A language name or repository path with an optional Git revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageSelector {
    pub name: String,
    pub revision: Option<String>,
}

impl LanguageSelector {
    /// Parse exactly the selector accepted by CLI `--lang`, without trimming.
    pub fn parse(input: &str) -> Result<Self, &'static str> {
        let (name, revision) = match input.split_once('@') {
            Some((name, revision)) => {
                if revision.is_empty()
                    || revision.contains('@')
                    || revision.chars().any(char::is_whitespace)
                    || revision.chars().any(char::is_control)
                {
                    return Err("expected language@revision with a nonempty revision");
                }

                let revision =
                    if revision.len() == 40 && revision.bytes().all(|c| c.is_ascii_hexdigit()) {
                        revision.to_ascii_lowercase()
                    } else {
                        revision.to_owned()
                    };
                (name, Some(revision))
            }
            None => (input, None),
        };
        if name.split('/').any(|part| {
            part.is_empty()
                || matches!(part, "." | "..")
                || part.contains('\\')
                || part.chars().any(char::is_control)
        }) {
            return Err(
                "expected language name or owner/repo[/grammar-directory] without empty or traversal components",
            );
        }

        Ok(Self {
            name: name.to_owned(),
            revision,
        })
    }
}
