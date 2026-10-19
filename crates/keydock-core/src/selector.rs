use crate::models::{EffectiveSelectors, SelectorProfile};

/// Inheritance rule for one selector slot:
/// - site `None` -> inherit the global value;
/// - site `Some("")` -> explicitly disabled;
/// - site `Some(selector)` -> use the site value.
pub fn resolve_field(global: Option<&str>, site: Option<&str>) -> Option<String> {
    match site {
        None => global
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        Some("") => None,
        Some(value) => {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_owned())
        }
    }
}

pub fn resolve(global: &SelectorProfile, site: &SelectorProfile) -> EffectiveSelectors {
    EffectiveSelectors {
        frame: resolve_field(global.frame.as_deref(), site.frame.as_deref()),
        username: resolve_field(global.username.as_deref(), site.username.as_deref()),
        password: resolve_field(global.password.as_deref(), site.password.as_deref()),
        captcha: resolve_field(global.captcha.as_deref(), site.captcha.as_deref()),
        submit: resolve_field(global.submit.as_deref(), site.submit.as_deref()),
        success: resolve_field(global.success.as_deref(), site.success.as_deref()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(values: [Option<&str>; 6]) -> SelectorProfile {
        SelectorProfile {
            frame: values[0].map(str::to_owned),
            username: values[1].map(str::to_owned),
            password: values[2].map(str::to_owned),
            captcha: values[3].map(str::to_owned),
            submit: values[4].map(str::to_owned),
            success: values[5].map(str::to_owned),
        }
    }

    #[test]
    fn null_inherits_global() {
        let global = profile([None, Some("#user"), Some("#pass"), None, Some("#go"), None]);
        let site = profile([None, None, None, None, None, None]);
        let resolved = resolve(&global, &site);
        assert_eq!(resolved.username.as_deref(), Some("#user"));
        assert_eq!(resolved.password.as_deref(), Some("#pass"));
        assert_eq!(resolved.submit.as_deref(), Some("#go"));
        assert_eq!(resolved.frame, None);
    }

    #[test]
    fn empty_string_disables_slot() {
        let global = profile([
            Some("#frame"),
            Some("#user"),
            Some("#pass"),
            Some("#cap"),
            Some("#go"),
            Some("#ok"),
        ]);
        let site = profile([Some(""), Some(""), None, Some(""), Some(""), Some("")]);
        let resolved = resolve(&global, &site);
        assert_eq!(resolved.frame, None);
        assert_eq!(resolved.username, None);
        assert_eq!(resolved.password.as_deref(), Some("#pass"));
        assert_eq!(resolved.captcha, None);
        assert_eq!(resolved.submit, None);
        assert_eq!(resolved.success, None);
    }

    #[test]
    fn site_value_wins_and_is_trimmed() {
        let global = profile([None, Some("#global"), None, None, None, None]);
        let site = profile([None, Some("  #site  "), None, None, None, None]);
        let resolved = resolve(&global, &site);
        assert_eq!(resolved.username.as_deref(), Some("#site"));
    }

    #[test]
    fn whitespace_only_site_value_disables_slot() {
        let global = profile([None, Some("#global"), None, None, None, None]);
        let site = profile([None, Some("   "), None, None, None, None]);
        let resolved = resolve(&global, &site);
        assert_eq!(resolved.username, None);
    }
}
