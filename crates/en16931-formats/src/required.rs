//! Which rule requires a term, and so whether a reader may report it
//! [`Kind::Missing`](crate::report::Kind::Missing): only when the declared
//! profile runs that rule.

use en16931::profiles::Declaration;

/// `(business term, the core rule that requires it)`.
pub(crate) const REQUIRED_BY: &[(&str, &str)] = &[
    ("BT-106", "BR-12"),
    ("BT-109", "BR-13"),
    ("BT-112", "BR-14"),
    ("BT-115", "BR-15"),
    ("BT-126", "BR-21"),
    ("BT-129", "BR-22"),
    ("BT-130", "BR-23"),
    ("BT-131", "BR-24"),
    ("BT-146", "BR-26"),
    ("BT-92", "BR-31"),
    ("BT-95", "BR-32"),
    ("BT-99", "BR-36"),
    ("BT-102", "BR-37"),
    ("BT-136", "BR-41"),
    ("BT-141", "BR-43"),
    ("BT-116", "BR-45"),
    ("BT-117", "BR-46"),
    ("BT-118", "BR-47"),
    ("BT-122", "BR-52"),
    ("BT-25", "BR-55"),
    ("BT-151", "BR-CO-04"),
];

/// The rule requiring the term a path like `BG-23[1]/BT-117` ends in.
pub(crate) fn rule_for(term: &str) -> Option<&'static str> {
    let bt = term.rsplit('/').next().unwrap_or(term);
    REQUIRED_BY.iter().find(|(t, _)| *t == bt).map(|(_, r)| *r)
}

/// Whether a document declaring `bt24` must carry `term`: its rule runs under
/// the declared profile. An unknown or absent declaration is held to the core,
/// as `receive` holds it (every core rule runs).
pub(crate) fn is_required(bt24: Option<&str>, term: &str) -> bool {
    let Some(rule) = rule_for(term) else {
        return true;
    };
    match en16931::profiles::classify(bt24) {
        Declaration::Known(p) => p.core.runs(rule),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every pair names a registered core rule, and each rule's own text names
    /// its term — so the table cannot drift from the registry.
    #[test]
    fn every_pair_is_a_rule_that_names_its_term() {
        for (bt, rule) in REQUIRED_BY {
            let r = en16931::validation::rules::CORE
                .iter()
                .find(|r| r.id.as_str() == *rule)
                .unwrap_or_else(|| panic!("{rule} is not a core rule"));
            assert!(
                r.text.contains(&format!("({bt})")),
                "{rule}'s text does not name {bt}: {}",
                r.text
            );
        }
    }

    #[test]
    fn minimum_lawfully_lacks_bt_106_and_the_core_does_not() {
        let minimum = Some(en16931::profiles::ZUGFERD_MINIMUM_ID);
        assert!(!is_required(minimum, "BG-22/BT-106"));
        assert!(is_required(None, "BG-22/BT-106"));
        assert!(is_required(
            Some("urn:cen.eu:en16931:2017"),
            "BG-23[1]/BT-117"
        ));
    }
}
