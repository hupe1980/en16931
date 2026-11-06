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

/// The rule requiring the term a path like `BG-23[0]/BT-117` ends in.
fn rule_for(term: &str) -> Option<&'static str> {
    let bt = term.rsplit('/').next().unwrap_or(term);
    REQUIRED_BY.iter().find(|(t, _)| *t == bt).map(|(_, r)| *r)
}

/// Why a reader reports `term`, a path like `BG-23[0]/BT-117`, as
/// [`Kind::Missing`](crate::report::Kind::Missing) when it is absent; `None`
/// when nothing requires it there.
///
/// A rule requires its term only when the declared profile runs it; an
/// unknown or absent declaration is held to the core, as `receive` holds it.
/// Beneath a sub-line (`BG-DEX-01`) no line rule applies: CEN binds `BR-21` …
/// `BR-26`, `BR-41`, `BR-43` and `BR-CO-04` to `cac:InvoiceLine |
/// cac:CreditNoteLine` and its allowances. There UBL's schema still requires
/// BT-126, BT-131 and an allowance's amount; BT-129, BT-130, BT-146 and
/// BT-151 are optional in the model.
pub(crate) fn requirement(bt24: Option<&str>, term: &str) -> Option<String> {
    let rule = rule_for(term);
    if term.contains("BG-DEX-01[") {
        return rule
            .filter(|r| matches!(*r, "BR-21" | "BR-24" | "BR-41" | "BR-43"))
            .map(|_| "the UBL schema requires it".to_owned());
    }
    let Some(rule) = rule else {
        return Some("the model requires it".to_owned());
    };
    let runs = match en16931::profiles::classify(bt24) {
        Declaration::Known(p) => p.core.runs(rule),
        _ => true,
    };
    runs.then(|| format!("{rule} requires it"))
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
        assert_eq!(requirement(minimum, "BG-22/BT-106"), None);
        assert_eq!(
            requirement(None, "BG-22/BT-106").as_deref(),
            Some("BR-12 requires it")
        );
        assert!(requirement(Some("urn:cen.eu:en16931:2017"), "BG-23[0]/BT-117").is_some());
    }

    #[test]
    fn no_line_rule_holds_beneath_a_sub_line() {
        let sub = |t: &str| requirement(None, &format!("BG-25[0]/BG-DEX-01[1]/{t}"));
        for optional in ["BT-129", "BT-130", "BT-146", "BT-151"] {
            assert_eq!(sub(optional), None, "{optional}");
        }
        for schema in ["BT-126", "BT-131", "BG-27[0]/BT-136", "BG-28[0]/BT-141"] {
            assert_eq!(
                sub(schema).as_deref(),
                Some("the UBL schema requires it"),
                "{schema}"
            );
        }
        assert_eq!(
            requirement(None, "BG-25[0]/BT-129").as_deref(),
            Some("BR-22 requires it")
        );
    }
}
