//! Text shown to people, in their own language when a translation for
//! it is installed.
//!
//! Every message goes through one of these functions, so the template
//! for translators can be made with `xgettext`. Messages with values in
//! them name each one in braces, such as `Imported {count} books`, so a
//! translation can put them in whatever order its language needs.

use gtk::glib;

/// The translation domain, which names the catalogue files.
pub const DOMAIN: &str = "mylongboxdb";

/// `msgid` in the reader's language.
pub fn gettext(msgid: &str) -> String {
    glib::dgettext(None, msgid).into()
}

/// The singular or plural form, as the reader's language counts `n`.
pub fn ngettext(singular: &str, plural: &str, n: usize) -> String {
    let n = std::ffi::c_ulong::try_from(n).unwrap_or(std::ffi::c_ulong::MAX);
    glib::dngettext(None, singular, plural, n).into()
}

/// `msgid` in the reader's language, where the same English means
/// different things in different places; `context` tells them apart.
pub fn pgettext(context: &str, msgid: &str) -> String {
    glib::dpgettext2(None, context, msgid).into()
}

/// `template` with each `{name}` replaced by its value. A name not
/// given is left as it is, so a slip in a translation shows rather than
/// losing text.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}').and_then(|close| {
            let name = &after[..close];
            values
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, value)| (close, *value))
        }) {
            Some((close, value)) => {
                out.push_str(value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Marks `msgid` for translation where it cannot be looked up yet,
/// such as in a `const` table; look it up with [`gettext`] where it is
/// shown.
pub const fn gettext_noop(msgid: &str) -> &str {
    msgid
}

/// [`gettext`], then [`fill`].
pub fn gettext_f(msgid: &str, values: &[(&str, &str)]) -> String {
    fill(&gettext(msgid), values)
}

/// [`ngettext`], then [`fill`].
pub fn ngettext_f(singular: &str, plural: &str, n: usize, values: &[(&str, &str)]) -> String {
    fill(&ngettext(singular, plural, n), values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_fill_their_places_in_any_order() {
        assert_eq!(
            fill(
                "Imported {count} books into {name}",
                &[("name", "Comics"), ("count", "3")]
            ),
            "Imported 3 books into Comics"
        );
        assert_eq!(fill("{a}{a}", &[("a", "x")]), "xx");
        // Unknown names and stray braces are kept, values are not
        // filled a second time.
        assert_eq!(fill("{nope} {", &[("a", "x")]), "{nope} {");
        assert_eq!(fill("{a}", &[("a", "{a}")]), "{a}");
        assert_eq!(fill("no values", &[]), "no values");
    }

    #[test]
    fn untranslated_messages_come_back_unchanged() {
        assert_eq!(gettext("Export a Copy"), "Export a Copy");
        assert_eq!(ngettext("{n} book", "{n} books", 1), "{n} book");
        assert_eq!(ngettext("{n} book", "{n} books", 2), "{n} books");
        assert_eq!(
            ngettext_f("{n} book", "{n} books", 5, &[("n", "5")]),
            "5 books"
        );
        assert_eq!(pgettext("verb", "Open"), "Open");
    }
}
