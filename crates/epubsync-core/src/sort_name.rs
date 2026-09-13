//! Makes an author sort name from a display name: the surname, a comma,
//! then the given names and any suffix. "Ursula K. Le Guin" becomes
//! "Le Guin, Ursula K.". A single word such as "Voltaire" stays as it is.
//!
//! The split comes from the `human_name` crate, which knows surname
//! particles and suffixes. It returns its strings in NFD form, so the
//! result is recomposed to NFC.

use unicode_normalization::UnicodeNormalization;

pub fn sort_name(display: &str) -> String {
    let display = display.trim();
    let Some(name) = human_name::Name::parse(display) else {
        return display.to_string();
    };
    let mut rest: Vec<String> = Vec::new();
    if let Some(given) = name.given_name() {
        rest.push(given.to_string());
    }
    if let Some(middle) = name.middle_names() {
        rest.extend(middle.iter().map(|m| m.to_string()));
    } else if let Some(initials) = name.middle_initials() {
        rest.extend(initials.chars().map(|c| format!("{c}.")));
    }
    if let Some(suffix) = name.generational_suffix() {
        rest.push(suffix.to_string());
    }
    let sorted = if rest.is_empty() {
        name.surname().to_string()
    } else {
        format!("{}, {}", name.surname(), rest.join(" "))
    };
    sorted.nfc().collect()
}

#[cfg(test)]
mod tests {
    use super::sort_name;

    #[test]
    fn moves_the_surname_first() {
        assert_eq!(sort_name("Ursula K. Le Guin"), "Le Guin, Ursula K.");
        assert_eq!(
            sort_name("Martin Luther King Jr."),
            "King, Martin Luther Jr."
        );
        assert_eq!(sort_name("Ludwig van Beethoven"), "van Beethoven, Ludwig");
        assert_eq!(sort_name("Voltaire"), "Voltaire");
    }

    #[test]
    fn recomposes_to_nfc() {
        let sorted = sort_name("Émile Zola");
        assert_eq!(sorted, "Zola, Émile");
        assert_eq!(sorted.chars().count(), "Zola, Émile".chars().count());
    }
}
