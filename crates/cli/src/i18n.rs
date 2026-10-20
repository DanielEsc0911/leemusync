//! Fluent strings for human output (i18n §Rust strings). `--json` never goes through here.

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};

const EN: &str = include_str!("../../../locales/en/leemusync.ftl");
const ES: &str = include_str!("../../../locales/es/leemusync.ftl");

pub struct I18n {
    bundle: FluentBundle<FluentResource>,
}

impl I18n {
    pub fn from_env() -> Self {
        let lang = ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"]
            .iter()
            .filter_map(|k| std::env::var(k).ok())
            .find(|v| !v.is_empty())
            .unwrap_or_default();
        Self::new(if lang.starts_with("es") { "es" } else { "en" })
    }

    pub fn new(lang: &str) -> Self {
        let (text, id) = if lang == "es" { (ES, "es") } else { (EN, "en") };
        let langid: unic_langid::LanguageIdentifier = id.parse().unwrap_or_default();
        let mut bundle = FluentBundle::new(vec![langid]);
        bundle.set_use_isolating(false);
        let resource =
            FluentResource::try_new(text.to_owned()).unwrap_or_else(|(partial, _)| partial);
        bundle.add_resource_overriding(resource);
        Self { bundle }
    }

    /// The message, or `None` if the key doesn't exist.
    pub fn get(&self, key: &str, args: &[(&str, FluentValue<'static>)]) -> Option<String> {
        let message = self.bundle.get_message(key)?;
        let pattern = message.value()?;
        let mut fargs = FluentArgs::new();
        for (k, v) in args {
            fargs.set(*k, v.clone());
        }
        let mut errors = Vec::new();
        Some(
            self.bundle
                .format_pattern(pattern, Some(&fargs), &mut errors)
                .into_owned(),
        )
    }

    /// The message; the key itself if it's missing (tests and `i18n-check` keep that from happening).
    pub fn t(&self, key: &str, args: &[(&str, FluentValue<'static>)]) -> String {
        self.get(key, args).unwrap_or_else(|| key.to_owned())
    }
}

/// Shorthand for a string argument.
pub fn s(v: impl Into<String>) -> FluentValue<'static> {
    FluentValue::from(v.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_files_parse_and_plurals_work() {
        assert!(FluentResource::try_new(EN.to_owned()).is_ok());
        assert!(FluentResource::try_new(ES.to_owned()).is_ok());
        let en = I18n::new("en");
        assert_eq!(en.t("history-head", &[]), "(latest)");
        let one = en.t(
            "scan-save",
            &[
                ("game", s("101c9400")),
                ("account", s("80000001")),
                ("files", FluentValue::from(1)),
            ],
        );
        assert!(one.ends_with("1 file"), "{one}");
        let es = I18n::new("es");
        let many = es.t(
            "scan-save",
            &[
                ("game", s("g")),
                ("account", s("a")),
                ("files", FluentValue::from(3)),
            ],
        );
        assert!(many.ends_with("3 archivos"), "{many}");
    }
}
