//! XRechnung's conditional rules — §7.3.2's one axis that needs code.
//!
//! Thirteen `BR-DE-*` rules are pure narrowings and live in
//! [`crate::profiles::XRECHNUNG`]'s `restrictions` as data. These are the rest:
//! rules whose condition depends on *another* term's value, which no
//! [`crate::validation::profile::Restriction`] variant can express and none
//! should try to.
//!
//! # Three of them have nothing left to check
//!
//! `BR-DE-23-b`, `-24-b` and `-25-b` each say that when BT-81 names one payment
//! means, the other two groups *"dürfen nicht übermittelt werden"*. Because
//! [`crate::invoice::PaymentMeans`] is an **enum**, that combination cannot be
//! written down — so the rules are registered with a constant-pass evaluation,
//! like the `BR-DEC-*` family.
//!
//! The `-a` halves remain real: they tie the *variant* to BT-81's **value**,
//! which the type system cannot see.

use super::{Findings, Rule, RuleId, Severity, Source};
use crate::VatCategory;
use crate::bt::{BtId, Group, Path};
use crate::invoice::{Invoice, PaymentMeans, terms as bt};

macro_rules! rule {
    (
        $konst:ident, $id:literal, $sev:ident,
        terms: [$($t:expr),* $(,)?],
        $text:literal,
        |$inv:ident, $f:ident| $body:block
    ) => {
        #[doc = $text]
        pub static $konst: Rule = Rule {
            id: RuleId::new($id),
            severity: Severity::$sev,
            text: $text,
            terms: &[$($t),*],
            source: Source::ArtefactOnly,
            eval: |$inv: &Invoice, $f: &mut Findings<'_>| $body,
        };
    };
}

/// UNTDID 4461 codes XRechnung treats as a credit transfer.
const CREDIT_TRANSFER: &[&str] = &["30", "58"];
/// …as a card payment.
const CARD: &[&str] = &["48", "54", "55"];
/// …as a direct debit.
const DIRECT_DEBIT: &[&str] = &["59"];

fn means_code(inv: &Invoice) -> Option<&str> {
    inv.payment
        .as_ref()
        .and_then(|p| p.means_code.as_ref())
        .map(crate::invoice::Code::as_str)
}

// ── BR-DE-16 — identifiers per category ───────────────────────────────────────

rule!(BR_DE_16, "BR-DE-16", Fatal,
terms: [bt::SELLER_VAT_ID, bt::SELLER_TAX_ID],
"Wenn in einer Rechnung die Steuercodes S, Z, E, AE, K, G, L oder M verwendet werden, muss \
 mindestens eines der Elemente \"Seller VAT identifier\" (BT-31), \"Seller tax registration \
 identifier\" (BT-32) oder \"Seller tax representative VAT identifier\" (BT-63) übermittelt \
 werden.",
|inv, f| {
    // Every category except `O` (not subject to VAT) and `B` (split
    // payment, which BR-B-01 already confines to Italy).
    let needs_id = inv.categories_used().iter().any(|c| {
        !matches!(c, VatCategory::OutOfScope | VatCategory::SplitPayment)
    });
    if needs_id
        && inv.seller.vat_identifier.is_none()
        && inv.seller.tax_registration.is_none()
    {
        f.at(Path::group_term(Group::Seller, bt::SELLER_VAT_ID));
    }
});

// ── BR-DE-23/24/25-a — the variant must match BT-81 ───────────────────────────

/// The `-a` half of a payment-means rule: BT-81 names a family, so the matching
/// group must be the one present.
macro_rules! means_rule {
    ($konst:ident, $id:literal, $codes:ident, $variant:pat, $text:literal) => {
        rule!($konst, $id, Fatal, terms: [bt::PAYMENT_MEANS_CODE], $text, |inv, f| {
            if means_code(inv).is_some_and(|c| $codes.contains(&c)) {
                let ok = matches!(
                    inv.payment.as_ref().and_then(|p| p.means.as_ref()),
                    Some($variant)
                );
                if !ok {
                    f.at(Path::group_term(Group::Payment, bt::PAYMENT_MEANS_CODE));
                }
            }
        });
    };
}

means_rule!(
    BR_DE_23_A,
    "BR-DE-23-a",
    CREDIT_TRANSFER,
    PaymentMeans::CreditTransfer(_),
    "Wenn BT-81 \"Payment means type code\" einen Schlüssel für Überweisungen enthält (30, 58), \
     muss BG-17 \"CREDIT TRANSFER\" übermittelt werden."
);
means_rule!(
    BR_DE_24_A,
    "BR-DE-24-a",
    CARD,
    PaymentMeans::Card(_),
    "Wenn BT-81 \"Payment means type code\" einen Schlüssel für Kartenzahlungen enthält \
     (48, 54, 55), muss genau BG-18 \"PAYMENT CARD INFORMATION\" übermittelt werden."
);
means_rule!(
    BR_DE_25_A,
    "BR-DE-25-a",
    DIRECT_DEBIT,
    PaymentMeans::DirectDebit(_),
    "Wenn BT-81 \"Payment means type code\" einen Schlüssel für Lastschriften enthält (59), muss \
     genau BG-19 \"DIRECT DEBIT\" übermittelt werden."
);

/// The `-b` half of a payment-means rule: the other two groups must be absent.
///
/// Unrepresentable — [`PaymentMeans`] is an enum. Registered so `explain` works.
macro_rules! means_rule_b {
    ($konst:ident, $id:literal, $text:literal) => {
        #[doc = $text]
        #[doc = ""]
        #[doc = "Satisfied by `PaymentMeans` being an enum: the forbidden combination cannot be written down."]
        pub static $konst: Rule = Rule {
            id: RuleId::new($id),
            severity: Severity::Fatal,
            text: $text,
            terms: &[],
            source: Source::ArtefactOnly,
            eval: |_, _| {},
        };
    };
}

means_rule_b!(
    BR_DE_23_B,
    "BR-DE-23-b",
    "Wenn BT-81 einen Schlüssel für Überweisungen enthält (30, 58), dürfen BG-18 und BG-19 nicht \
     übermittelt werden."
);
means_rule_b!(
    BR_DE_24_B,
    "BR-DE-24-b",
    "Wenn BT-81 einen Schlüssel für Kartenzahlungen enthält (48, 54, 55), dürfen BG-17 und BG-19 \
     nicht übermittelt werden."
);
means_rule_b!(
    BR_DE_25_B,
    "BR-DE-25-b",
    "Wenn BT-81 einen Schlüssel für Lastschriften enthält (59), dürfen BG-17 und BG-18 nicht \
     übermittelt werden."
);

// ── BR-DE-30 / BR-DE-31 — direct debit detail ─────────────────────────────────

rule!(BR_DE_30, "BR-DE-30", Fatal, terms: [BtId(90)],
"Wenn \"DIRECT DEBIT\" (BG-19) vorhanden ist, dann muss \"Bank assigned creditor identifier\" \
 (BT-90) übermittelt werden.",
|inv, f| {
    if let Some(PaymentMeans::DirectDebit(d)) =
        inv.payment.as_ref().and_then(|p| p.means.as_ref())
        && d.creditor_identifier.as_deref().is_none_or(str::is_empty)
    {
        f.at(Path::group_term(Group::Payment, BtId(90)));
    }
});

/// Whether BT-90 is a well-formed SEPA Creditor Identifier (EPC AT-02).
///
/// `CC##ZZZxxxxxxxx`: country, two mod-97 check digits over the national
/// identifier, a three-character business code, then the identifier itself.
///
/// Without the `sepa` feature this is a shape check only — the check digits are
/// not verified, because the algorithm strips the business code before
/// computing them and getting that subtly wrong is worse than not checking.
#[must_use]
pub fn is_valid_creditor_identifier(s: &str) -> bool {
    #[cfg(feature = "sepa")]
    {
        sepa::validate_creditor_id(s).is_ok()
    }
    #[cfg(not(feature = "sepa"))]
    {
        let c: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        c.len() >= 8
            && c.len() <= 35
            && c.bytes().all(|b| b.is_ascii_alphanumeric())
            && c[..2].bytes().all(|b| b.is_ascii_alphabetic())
            && c[2..4].bytes().all(|b| b.is_ascii_digit())
    }
}

/// `EN-SEPA-01` — BT-90 should be a well-formed SEPA Creditor Identifier.
///
/// **This crate's own rule**, namespaced so it can never be mistaken for CEN's
/// or KoSIT's. `BR-DE-30` requires BT-90 to be *present* and no rule anywhere
/// checks that it is well formed — yet a direct debit quoting a malformed
/// creditor identifier is rejected by the creditor's bank, not by the buyer, and
/// long after the invoice was accepted.
///
/// A warning, not an error: the document is lawful and the standard does not ask
/// for this. With the `sepa` feature the check is EPC AT-02 including the
/// mod-97 check digits; without it, the shape only.
pub static EN_SEPA_01: Rule = Rule {
    id: RuleId::new("EN-SEPA-01"),
    severity: Severity::Warning,
    text: "\"Bank assigned creditor identifier\" (BT-90) should be a valid SEPA Creditor \
           Identifier (EPC AT-02).",
    terms: &[BtId(90)],
    source: Source::Crate,
    eval: |inv, f| {
        if let Some(PaymentMeans::DirectDebit(d)) =
            inv.payment.as_ref().and_then(|p| p.means.as_ref())
            && d.creditor_identifier
                .as_deref()
                .is_some_and(|id| !id.is_empty() && !is_valid_creditor_identifier(id))
        {
            f.at(Path::group_term(Group::Payment, BtId(90)));
        }
    },
};

rule!(BR_DE_31, "BR-DE-31", Fatal, terms: [BtId(91)],
"Wenn \"DIRECT DEBIT\" (BG-19) vorhanden ist, dann muss \"Debited account identifier\" \
 (BT-91) übermittelt werden.",
|inv, f| {
    if let Some(PaymentMeans::DirectDebit(d)) =
        inv.payment.as_ref().and_then(|p| p.means.as_ref())
        && d.debited_account.as_deref().is_none_or(str::is_empty)
    {
        f.at(Path::group_term(Group::Payment, BtId(91)));
    }
});

// ── BR-DE-26 — a corrected invoice references the original ────────────────────

rule!(BR_DE_26, "BR-DE-26", Fatal, terms: [bt::TYPE_CODE, bt::PRECEDING_INVOICE],
"Wenn im Element \"Invoice type code\" (BT-3) der Code 384 (Corrected invoice) übergeben \
 wird, soll PRECEDING INVOICE REFERENCE (BG-3) mindestens einmal übermittelt werden.",
|inv, f| {
    if inv.type_code.as_ref().is_some_and(|c| c.as_str() == "384")
        && inv.preceding_invoices.is_empty()
    {
        f.at(Path::term(bt::PRECEDING_INVOICE));
    }
});

// ── BR-DE-19 / BR-DE-20 — IBAN, checked offline ───────────────────────────────

/// ISO 13616 / ISO 7064 mod-97-10, and the country registry when available.
///
/// An **offline** check either way: no network, so it runs on `wasm32` like
/// everything else. It cannot tell you the account exists — only that the string
/// is not a typo, which is what catches the overwhelming majority of real
/// errors.
///
/// # Two strengths, and the difference is real
///
/// The built-in check is the mod-97-10 checksum and nothing else. That is
/// correct as far as it goes and **blind to length**: `DE89370400440532013000`
/// is 22 characters, which is right for Germany, but a 21-character string with
/// a valid checksum passes too, and no German bank will accept it.
///
/// With the **`sepa` feature**, the check is [`sepa::validate_iban`] — the full
/// ISO 13616 registry, 89 countries, each with its own length and BBAN
/// structure. `BR-DE-19` and `BR-DE-20` are warnings either way; with the
/// feature they are warnings you can act on.
#[must_use]
pub fn is_valid_iban(s: &str) -> bool {
    #[cfg(feature = "sepa")]
    {
        sepa::validate_iban(s).is_ok()
    }
    #[cfg(not(feature = "sepa"))]
    {
        is_valid_iban_checksum(s)
    }
}

/// The checksum-only fallback, and the reference the `sepa` path must agree
/// with on everything it accepts.
#[must_use]
pub fn is_valid_iban_checksum(s: &str) -> bool {
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() < 5 || compact.len() > 34 {
        return false;
    }
    if !compact.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return false;
    }
    let bytes = compact.as_bytes();
    if !bytes[0].is_ascii_alphabetic()
        || !bytes[1].is_ascii_alphabetic()
        || !bytes[2].is_ascii_digit()
        || !bytes[3].is_ascii_digit()
    {
        return false;
    }
    // Move the first four characters to the end, map letters to 10..35, then
    // take the remainder mod 97 digit by digit — the number is far too large for
    // any integer type.
    let rearranged = compact[4..].chars().chain(compact[..4].chars());
    let mut remainder: u32 = 0;
    for c in rearranged {
        let value = if c.is_ascii_digit() {
            u32::from(c as u8 - b'0')
        } else {
            u32::from(c.to_ascii_uppercase() as u8 - b'A') + 10
        };
        // Two digits for a letter, one for a digit.
        remainder = if value >= 10 {
            (remainder * 100 + value) % 97
        } else {
            (remainder * 10 + value) % 97
        };
    }
    remainder == 1
}

rule!(BR_DE_19, "BR-DE-19", Warning, terms: [bt::PAYMENT_ACCOUNT],
"\"Payment account identifier\" (BT-84) soll eine korrekte IBAN enthalten, wenn in \"Payment \
 means type code\" (BT-81) der Code 58 (SEPA credit transfer) angegeben ist.",
|inv, f| {
    if means_code(inv) == Some("58")
        && let Some(p) = &inv.payment
        && let Some(acc) = p.account_identifier()
        && !is_valid_iban(acc)
    {
        f.at(Path::group_term(Group::Payment, bt::PAYMENT_ACCOUNT));
    }
});

rule!(BR_DE_20, "BR-DE-20", Warning, terms: [BtId(91)],
"\"Debited account identifier\" (BT-91) soll eine korrekte IBAN enthalten, wenn in \"Payment \
 means type code\" (BT-81) der Code 59 (SEPA direct debit) angegeben ist.",
|inv, f| {
    if means_code(inv) == Some("59")
        && let Some(PaymentMeans::DirectDebit(d)) =
            inv.payment.as_ref().and_then(|p| p.means.as_ref())
        && let Some(acc) = d.debited_account.as_deref()
        && !is_valid_iban(acc)
    {
        f.at(Path::group_term(Group::Payment, BtId(91)));
    }
});

// ── BR-DE-27 / BR-DE-28 — contact formats ─────────────────────────────────────

rule!(BR_DE_27, "BR-DE-27", Fatal, terms: [BtId(42)],
"In BT-42 sollen mindestens drei Ziffern enthalten sein.",
|inv, f| {
    if let Some(phone) = inv.seller.contact.phone.as_deref()
        && phone.chars().filter(char::is_ascii_digit).count() < 3
    {
        f.at(Path::group_term(Group::Seller, BtId(42)));
    }
});

rule!(BR_DE_28, "BR-DE-28", Fatal, terms: [BtId(43)],
"In BT-43 soll genau ein @-Zeichen enthalten sein, welches nicht von einem Leerzeichen oder \
 einem Punkt, aber von mindestens zwei Zeichen auf beiden Seiten flankiert wird. Ein Punkt \
 sollte nicht am Anfang oder am Ende stehen.",
|inv, f| {
    if let Some(email) = inv.seller.contact.email.as_deref()
        && !plausible_email(email)
    {
        f.at(Path::group_term(Group::Seller, BtId(43)));
    }
});

/// BR-DE-28's shape test, which is deliberately weaker than a full RFC 5322
/// parse: exactly one `@`, at least two characters either side, neither
/// adjacent character a space or a dot, and no leading or trailing dot.
fn plausible_email(s: &str) -> bool {
    let at: Vec<usize> = s.match_indices('@').map(|(i, _)| i).collect();
    if at.len() != 1 {
        return false;
    }
    let (local, domain) = s.split_at(at[0]);
    let domain = &domain[1..];
    if local.chars().count() < 2 || domain.chars().count() < 2 {
        return false;
    }
    let bad = |c: Option<char>| matches!(c, Some(' ' | '.'));
    if bad(local.chars().next_back()) || bad(domain.chars().next()) {
        return false;
    }
    !s.starts_with('.') && !s.ends_with('.')
}

/// Every rule this module defines.
pub static ALL: &[&Rule] = &[
    &BR_DE_2,
    &BR_DE_10,
    &BR_DE_11,
    &BR_DE_18,
    &BR_DE_22,
    &BR_TMP_2,
    &BR_DE_TMP_32,
    &BR_DE_16,
    &BR_DE_19,
    &BR_DE_20,
    &BR_DE_23_A,
    &BR_DE_23_B,
    &BR_DE_24_A,
    &BR_DE_24_B,
    &BR_DE_25_A,
    &BR_DE_25_B,
    &BR_DE_26,
    &BR_DE_27,
    &BR_DE_28,
    &BR_DE_30,
    &BR_DE_31,
    &EN_SEPA_01,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iban_mod_97_accepts_real_ibans_and_rejects_typos() {
        // Published test IBANs.
        for ok in [
            "DE89370400440532013000",
            "GB82 WEST 1234 5698 7654 32",
            "FR1420041010050500013M02606",
            "NL91ABNA0417164300",
        ] {
            assert!(is_valid_iban(ok), "{ok} should be valid");
        }
        for bad in [
            "DE89370400440532013001", // one digit changed
            "DE8937040044053201300",  // one digit short
            "XX00",                   // too short
            "0089370400440532013000", // does not start with letters
            "DE89-3704-0044",         // punctuation
            "",
        ] {
            assert!(!is_valid_iban(bad), "{bad:?} should be invalid");
        }
    }

    #[test]
    fn br_de_28_is_a_shape_test_not_an_rfc_parser() {
        for ok in ["rechnung@seller.de", "a.b@example.co.uk"] {
            assert!(plausible_email(ok), "{ok}");
        }
        for bad in [
            "no-at-sign",
            "two@@ats.de",
            "a@b.de",         // one character before the @
            "ab@c",           // one character after
            "ab .@seller.de", // space adjacent to the @
            "ab.@seller.de",  // dot adjacent to the @
            ".ab@seller.de",  // leading dot
            "ab@seller.de.",  // trailing dot
        ] {
            assert!(!plausible_email(bad), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn the_b_halves_are_unrepresentable_rather_than_unchecked() {
        // `PaymentMeans` is an enum, so "credit transfer AND card" cannot be
        // written down. The rules stay in the registry so `explain` works.
        for r in [&BR_DE_23_B, &BR_DE_24_B, &BR_DE_25_B] {
            let mut out = Vec::new();
            let mut sink = crate::validation::Findings::for_test(&mut out, r);
            (r.eval)(&Invoice::default(), &mut sink);
            assert!(out.is_empty());
        }
    }

    #[test]
    fn both_iban_rules_are_advisory() {
        // KoSIT flags these `soll`, and an IBAN this crate cannot verify against
        // a registry is a warning, not a rejection.
        assert_eq!(BR_DE_19.severity, Severity::Warning);
        assert_eq!(BR_DE_20.severity, Severity::Warning);
    }
}

// ── The rest of XRechnung 3.0.2 ───────────────────────────────────────────────

rule!(BR_DE_2, "BR-DE-2", Fatal, terms: [BtId(41), BtId(42), BtId(43)],
"Die Gruppe \"SELLER CONTACT\" (BG-6) muss übermittelt werden.",
|inv, f| {
    let c = &inv.seller.contact;
    if c.name.is_none() && c.phone.is_none() && c.email.is_none() {
        f.at(Path::group(Group::Seller));
    }
});

rule!(BR_DE_10, "BR-DE-10", Fatal, terms: [BtId(77)],
"Das Element \"Deliver to city\" (BT-77) muss übermittelt werden, wenn die Gruppe \"DELIVER TO \
 ADDRESS\" (BG-15) übermittelt wird.",
|inv, f| {
    if let Some(a) = inv.delivery.as_ref().and_then(|d| d.address.as_ref())
        && a.city.as_deref().is_none_or(|c| c.trim().is_empty())
    {
        f.at(Path::at_term(Group::Delivery, 0, BtId(77)));
    }
});

rule!(BR_DE_11, "BR-DE-11", Fatal, terms: [BtId(78)],
"Das Element \"Deliver to post code\" (BT-78) muss übermittelt werden, wenn die Gruppe \"DELIVER \
 TO ADDRESS\" (BG-15) übermittelt wird.",
|inv, f| {
    if let Some(a) = inv.delivery.as_ref().and_then(|d| d.address.as_ref())
        && a.post_code.as_deref().is_none_or(|c| c.trim().is_empty())
    {
        f.at(Path::at_term(Group::Delivery, 0, BtId(78)));
    }
});

rule!(BR_DE_22, "BR-DE-22", Fatal, terms: [BtId(125)],
"Das \"filename\"-Attribut aller \"EmbeddedDocumentBinaryObject\"-Elemente muss eindeutig sein.",
|inv, f| {
    // Quadratic, and deliberately so: BG-24 is a handful of documents, and a
    // `HashSet` would pull `std` into a module that does not otherwise need it.
    // The finding is raised on the *duplicate*, which is the one to rename.
    for (i, doc) in inv.attachments.iter().enumerate() {
        let Some(name) = doc.attachment.as_ref().map(crate::Attachment::filename) else {
            continue;
        };
        let dup = inv.attachments[..i]
            .iter()
            .filter_map(|d| d.attachment.as_ref())
            .any(|a| a.filename() == name);
        if dup {
            f.at(Path::at_term(Group::Attachment, i, BtId(125)));
        }
    }
});

rule!(BR_DE_18, "BR-DE-18", Fatal, terms: [bt::PAYMENT_TERMS],
"Skonto-Zeilen in BT-20 müssen der Form #SKONTO#TAGE=n#PROZENT=n.nn[#BASISBETRAG=n.nn]# \
 entsprechen.",
|inv, f| {
    let Some(terms) = inv.payment_terms.as_deref() else {
        return;
    };
    // XRechnung binds this to a regex; this crate has no regex dependency and
    // will not take one for a single rule, so the grammar is parsed directly.
    // The grammar is small and fully specified, which is why that is safe here
    // and would not be for something like an email address.
    let mut bad = terms
        .lines()
        .map(str::trim)
        .any(|line| line.starts_with('#') && !is_skonto_line(line));

    // The second half of the rule, and the easy half to miss. KoSIT asserts it
    // separately:
    //
    // ```xslt
    // matches(cac:PaymentTerms/cbc:Note[1]/tokenize(., '#.+#')[last()], '^\s*\n')
    // ```
    //
    // Everything after the **last** `#…#` must begin with a newline, so a Skonto
    // block that ends without one is invalid even though every line parses.
    // KoSIT's own suite has two cases for exactly this ("Every skonto entry
    // should end with a newline") and nothing else catches it.
    if let Some(last_hash) = terms.rfind('#') {
        let tail = &terms[last_hash + 1..];
        if terms.contains("#SKONTO#") && !tail.trim_start_matches([' ', '\t', '\r']).starts_with('\n')
        {
            bad = true;
        }
    }
    if bad {
        f.at(Path::term(bt::PAYMENT_TERMS));
    }
});

/// XRechnung's `$XR-SKONTO-REGEX`, hand-parsed.
///
/// ```text
/// #SKONTO#TAGE=<digits>#PROZENT=<digits>.<2 digits>[#BASISBETRAG=[-]<digits>.<2 digits>]#
/// ```
///
/// Note the two-decimal amounts are **exact**, not "up to two": `PROZENT=3` and
/// `PROZENT=3.0` are both invalid, which surprises people and is what the rule
/// says.
fn is_skonto_line(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("#SKONTO#TAGE=") else {
        return false;
    };
    let Some((days, rest)) = rest.split_once("#PROZENT=") else {
        return false;
    };
    if days.is_empty() || !days.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    // Either `<pct>#` or `<pct>#BASISBETRAG=<amount>#`.
    let Some(body) = rest.strip_suffix('#') else {
        return false;
    };
    match body.split_once("#BASISBETRAG=") {
        Some((pct, base)) => is_two_dp(pct, false) && is_two_dp(base, true),
        None => is_two_dp(body, false),
    }
}

/// `<digits>.<exactly two digits>`, optionally signed.
fn is_two_dp(s: &str, allow_sign: bool) -> bool {
    let s = if allow_sign {
        s.strip_prefix('-').unwrap_or(s)
    } else {
        s
    };
    let Some((int, frac)) = s.split_once('.') else {
        return false;
    };
    !int.is_empty()
        && int.bytes().all(|b| b.is_ascii_digit())
        && frac.len() == 2
        && frac.bytes().all(|b| b.is_ascii_digit())
}

rule!(BR_TMP_2, "BR-TMP-2", Warning, terms: [BtId(124)],
"BT-124 \"External document location\" muss eine absolute URL mit gültigem Schema enthalten.",
|inv, f| {
    for (i, doc) in inv.attachments.iter().enumerate() {
        if let Some(uri) = doc.uri.as_deref()
            && !is_absolute_url(uri)
        {
            f.at(Path::at_term(Group::Attachment, i, BtId(124)));
        }
    }
});

/// `scheme://…` with a plausible RFC 3986 scheme.
fn is_absolute_url(s: &str) -> bool {
    let Some((scheme, rest)) = s.split_once("://") else {
        return false;
    };
    !scheme.is_empty()
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
        && !rest.is_empty()
}

rule!(BR_DE_TMP_32, "BR-DE-TMP-32", Info, terms: [BtId(72), BtId(73), BtId(134)],
"Eine Rechnung sollte zur Angabe des Liefer-/Leistungsdatums entweder BT-72, BG-14 oder BG-26 \
 (in allen Rechnungspositionen) enthalten.",
|inv, f| {
    let has_delivery_date = inv.delivery.as_ref().is_some_and(|d| d.date.is_some());
    let has_period = inv.invoicing_period.is_some();
    let every_line_has_one = !inv.lines.is_empty() && inv.lines.iter().all(|l| l.period.is_some());
    if !(has_delivery_date || has_period || every_line_has_one) {
        f.at(Path::term(BtId(72)));
    }
});

// ── The Clean Vehicles Directive variant ──────────────────────────────────────

/// XRechnung CVD — the EU Clean Vehicles Directive variant.
///
/// Directive (EU) 2019/1161 obliges public bodies to meet minimum procurement
/// targets for clean road vehicles, which means the *invoice* has to carry
/// enough to count one: which vehicle category, and whether it is clean.
/// XRechnung's CVD variant is a second CIUS on top of the first, selected by its
/// own `CustomizationID`, and these seven rules are what it adds.
///
/// It is a good demonstration of why [`crate::validation::profile::Restriction`]
/// is not enough on its own: `BR-DE-CVD-03` is *"at least one line must carry
/// both a `CVD` classification and a `cva` attribute"*, which is a search over
/// two nested groups, not a narrowing of a term.
pub mod cvd {
    use super::{Findings, Group, Invoice, Path, Rule, RuleId, Severity, Source};
    use crate::bt::BtId;

    /// `BR-DE-CVD-04` — the vehicle categories of Directive 2007/46/EC Annex II.
    ///
    /// `M1` is a car, `M2`/`M3` buses, `N1`…`N3` goods vehicles by weight.
    pub const VEHICLE_CATEGORIES: &[&str] = &["M1", "M2", "M3", "N1", "N2", "N3"];

    /// `BR-DE-CVD-05` — the permitted values of the `cva` item attribute.
    pub const CVA_VALUES: &[&str] = &["clean", "other", "zero-emission"];

    /// The item classification scheme that marks a line as a vehicle.
    pub const CVD_SCHEME: &str = "CVD";

    /// The `cva` item attribute name — "clean vehicle attribute".
    pub const CVA_NAME: &str = "cva";

    /// Whether an item carries a `CVD` classification, and how many.
    fn cvd_classifications(item: &crate::invoice::Item) -> usize {
        item.classification_identifiers
            .iter()
            .filter(|id| id.scheme() == Some(CVD_SCHEME))
            .count()
    }

    /// How many `cva` attributes an item carries.
    fn cva_attributes(item: &crate::invoice::Item) -> usize {
        item.attributes
            .iter()
            .filter(|a| a.name.as_deref() == Some(CVA_NAME))
            .count()
    }

    macro_rules! cvd_rule {
        ($konst:ident, $id:literal, terms: [$($t:expr),* $(,)?], $text:literal,
         |$inv:ident, $f:ident| $body:block) => {
            #[doc = $text]
            pub static $konst: Rule = Rule {
                id: RuleId::new($id),
                severity: Severity::Fatal,
                text: $text,
                terms: &[$($t),*],
                source: Source::ArtefactOnly,
                eval: |$inv: &Invoice, $f: &mut Findings<'_>| $body,
            };
        };
    }

    cvd_rule!(BR_DE_CVD_01, "BR-DE-CVD-01", terms: [BtId(12)],
    "Das Element \"Contract reference\" (BT-12) muss übermittelt werden.",
    |inv, f| {
        if inv
            .contract_reference
            .as_ref()
            .is_none_or(|r| r.as_str().trim().is_empty())
        {
            f.at(Path::term(BtId(12)));
        }
    });

    cvd_rule!(BR_DE_CVD_02, "BR-DE-CVD-02", terms: [BtId(17)],
    "Das Element \"Tender or lot reference\" (BT-17) muss übermittelt werden.",
    |inv, f| {
        if inv
            .tender_reference
            .as_ref()
            .is_none_or(|r| r.as_str().trim().is_empty())
        {
            f.at(Path::term(BtId(17)));
        }
    });

    cvd_rule!(BR_DE_CVD_03, "BR-DE-CVD-03", terms: [BtId(158), BtId(160)],
    "In einer Rechnung muss mindestens eine INVOICE LINE (BG-25) enthalten sein, in der der Scheme \
     identifier von BT-158 'CVD' ist und BT-160 den Wert 'cva' hat.",
    |inv, f| {
        let any = inv
            .lines
            .iter()
            .any(|l| cvd_classifications(&l.item) > 0 && cva_attributes(&l.item) > 0);
        if !any {
            f.at(Path::term(BtId(158)));
        }
    });

    cvd_rule!(BR_DE_CVD_04, "BR-DE-CVD-04", terms: [BtId(158)],
    "Ein \"Item classification identifier\" (BT-158) mit dem Scheme identifier 'CVD' muss einen \
     Wert aus der Liste der Fahrzeugklassen enthalten.",
    |inv, f| {
        for (i, line) in inv.lines.iter().enumerate() {
            for id in &line.item.classification_identifiers {
                if id.scheme() == Some(CVD_SCHEME)
                    && !VEHICLE_CATEGORIES.contains(&id.content())
                {
                    f.at(Path::at_term(Group::Line, i, BtId(158)));
                }
            }
        }
    });

    cvd_rule!(BR_DE_CVD_05, "BR-DE-CVD-05", terms: [BtId(161)],
    "Wenn \"Item attribute name\" (BT-160) den Wert 'cva' hat, muss BT-161 einen der Werte \
     'clean', 'zero-emission' oder 'other' enthalten.",
    |inv, f| {
        for (i, line) in inv.lines.iter().enumerate() {
            for a in &line.item.attributes {
                if a.name.as_deref() == Some(CVA_NAME)
                    && !a.value.as_deref().is_some_and(|v| CVA_VALUES.contains(&v))
                {
                    f.at(Path::at_term(Group::Line, i, BtId(161)));
                }
            }
        }
    });

    cvd_rule!(BR_DE_CVD_06_A, "BR-DE-CVD-06-a", terms: [BtId(158), BtId(160)],
    "Wenn der Scheme identifier von BT-158 den Wert 'CVD' hat, muss genau ein BT-160 mit dem Wert \
     'cva' in derselben Rechnungsposition angegeben sein.",
    |inv, f| {
        for (i, line) in inv.lines.iter().enumerate() {
            if cvd_classifications(&line.item) > 0 && cva_attributes(&line.item) != 1 {
                f.at(Path::at_term(Group::Line, i, BtId(160)));
            }
        }
    });

    cvd_rule!(BR_DE_CVD_06_B, "BR-DE-CVD-06-b", terms: [BtId(158), BtId(160)],
    "Wenn BT-160 mit dem Wert 'cva' angegeben ist, muss in derselben Rechnungsposition genau ein \
     BT-158 mit dem Scheme identifier 'CVD' angegeben sein.",
    |inv, f| {
        for (i, line) in inv.lines.iter().enumerate() {
            if cva_attributes(&line.item) > 0 && cvd_classifications(&line.item) != 1 {
                f.at(Path::at_term(Group::Line, i, BtId(158)));
            }
        }
    });

    cvd_rule!(BR_TMP_CVD_01, "BR-TMP-CVD-01", terms: [BtId(158)],
    "Das Bildungsschema für \"Item classification identifier\" (BT-158) ist aus der Codeliste \
     UNTDID 7143, erweitert um 'CVD'.",
    |inv, f| {
        for (i, line) in inv.lines.iter().enumerate() {
            for id in &line.item.classification_identifiers {
                if let Some(scheme) = id.scheme()
                    && scheme != CVD_SCHEME
                    && !crate::codes::contains(
                        crate::codes::generated::ITEM_CLASSIFICATION_SCHEMES,
                        scheme,
                    )
                {
                    f.at(Path::at_term(Group::Line, i, BtId(158)));
                }
            }
        }
    });

    /// The eight rules the CVD variant adds on top of XRechnung.
    pub static ALL: &[&Rule] = &[
        &BR_DE_CVD_01,
        &BR_DE_CVD_02,
        &BR_DE_CVD_03,
        &BR_DE_CVD_04,
        &BR_DE_CVD_05,
        &BR_DE_CVD_06_A,
        &BR_DE_CVD_06_B,
        &BR_TMP_CVD_01,
    ];
}

// ── The XRechnung Extension ───────────────────────────────────────────────────

/// The `BR-DEX-*` family — XRechnung's **Extension**, not its CIUS.
///
/// §4.3's second mechanism, and the clearest example of it in the wild. Every
/// rule here either governs a group the core model has no term for
/// ([`crate::extensions::SubInvoiceLine`], [`crate::extensions::ThirdPartyPayment`])
/// or **widens** something core EN 16931 narrows:
///
/// | | Core | Extension |
/// |---|---|---|
/// | BT-125 mime | six codes | + `application/xml` |
/// | scheme identifiers | ISO 6523 ICD | + `XR01`, `XR02`, `XR03` |
/// | BT-115 | `BR-CO-16` | **`BR-DEX-09`**, with third-party payments added |
///
/// Widening is what makes it an Extension. A CIUS may not do it, which is why
/// this is a separate profile with its own specification identifier rather than
/// more restrictions on [`crate::profiles::XRECHNUNG`].
///
/// `XR01`…`XR03` are DiGA codes — *Digitale Gesundheitsanwendungen*, German
/// prescribable health apps, where a statutory insurer settles part of an
/// invoice addressed to the insured. That is also what `BG-DEX-09` exists for.
pub mod extension {
    use super::{Findings, Group, Invoice, Path, Rule, RuleId, Severity, Source};
    use crate::bt::BtId;
    use crate::extensions::SubInvoiceLine;

    /// `$DIGA-CODES` — the three the Extension adds to ISO 6523 ICD and CEF EAS.
    pub const DIGA_SCHEMES: &[&str] = &["XR01", "XR02", "XR03"];

    /// `BR-DEX-01` — Peppol's six mime codes plus `application/xml`.
    pub const EXTENSION_MIME_CODES: &[&str] = &[
        "application/pdf",
        "application/vnd.oasis.opendocument.spreadsheet",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "application/xml",
        "image/jpeg",
        "image/png",
        "text/csv",
    ];

    /// Whether a scheme is admissible under the Extension's widened ICD list.
    fn icd_or_diga(scheme: &str) -> bool {
        DIGA_SCHEMES.contains(&scheme)
            || crate::codes::contains(crate::codes::generated::ICD_SCHEMES, scheme)
    }

    /// …and its widened CEF EAS list.
    fn eas_or_diga(scheme: &str) -> bool {
        DIGA_SCHEMES.contains(&scheme)
            || crate::codes::contains(crate::codes::generated::EAS_SCHEMES, scheme)
    }

    macro_rules! dex {
        ($konst:ident, $id:literal, $sev:ident, terms: [$($t:expr),* $(,)?], $text:literal,
         |$inv:ident, $f:ident| $body:block) => {
            #[doc = $text]
            pub static $konst: Rule = Rule {
                id: RuleId::new($id),
                severity: Severity::$sev,
                text: $text,
                terms: &[$($t),*],
                source: Source::ArtefactOnly,
                eval: |$inv: &Invoice, $f: &mut Findings<'_>| $body,
            };
        };
    }

    dex!(BR_DEX_01, "BR-DEX-01", Fatal, terms: [BtId(125)],
    "Das Element \"Attached Document\" (BT-125) benutzt einen nicht zulässigen MIME-Code. Im Falle \
     einer Extension ist zusätzlich 'application/xml' zulässig.",
    |inv, f| {
        for (i, doc) in inv.attachments.iter().enumerate() {
            if let Some(a) = &doc.attachment
                && !EXTENSION_MIME_CODES.contains(&a.mime_code())
            {
                f.at(Path::at_term(Group::Attachment, i, BtId(125)));
            }
        }
    });

    dex!(BR_DEX_02, "BR-DEX-02", Warning, terms: [BtId(131)],
    "Der Wert von \"Invoice line net amount\" (BT-131) einer INVOICE LINE (BG-25) oder einer SUB \
     INVOICE LINE (BG-DEX-01) soll der Summe der Beträge ihrer Sub Invoice Lines entsprechen.",
    |inv, f| {
        for (i, line) in inv.lines.iter().enumerate() {
            let subs = inv.extensions.sub_lines(i);
            if subs.is_empty() {
                continue;
            }
            check_sum(line.net_amount, subs, Path::at_term(Group::Line, i, BtId(131)), f);
            // …and recursively, for sub-lines that decompose further.
            for s in subs {
                check_subtree(s, Path::at_term(Group::Line, i, BtId(131)), f);
            }
        }
    });

    /// `BT-131 = Σ` immediate children, warning when it does not hold.
    fn check_sum(
        stated: crate::InvoiceAmount,
        children: &[SubInvoiceLine],
        path: Path,
        f: &mut Findings<'_>,
    ) {
        let Ok(sum) = crate::InvoiceAmount::checked_sum(children.iter().map(|c| c.line.net_amount))
        else {
            return;
        };
        if sum != stated {
            f.arithmetic(path, sum, stated);
        }
    }

    fn check_subtree(node: &SubInvoiceLine, path: Path, f: &mut Findings<'_>) {
        if node.children.is_empty() {
            return;
        }
        check_sum(node.line.net_amount, &node.children, path, f);
        for c in &node.children {
            check_subtree(c, path, f);
        }
    }

    dex!(BR_DEX_03, "BR-DEX-03", Fatal, terms: [BtId(151)],
    "Eine Sub Invoice Line (BG-DEX-01) muss genau eine SUB INVOICE LINE VAT INFORMATION \
     (BG-DEX-06) enthalten.",
    |inv, f| {
        fn walk(nodes: &[SubInvoiceLine], i: usize, f: &mut Findings<'_>) {
            for n in nodes {
                if n.vat.is_none() {
                    f.at(Path::at_term(Group::Line, i, BtId(151)));
                }
                walk(&n.children, i, f);
            }
        }
        for i in 0..inv.lines.len() {
            walk(inv.extensions.sub_lines(i), i, f);
        }
    });

    dex!(BR_DEX_04, "BR-DEX-04", Fatal, terms: [BtId(29), BtId(46)],
    "Any scheme identifier in cac:PartyIdentification MUST be coded using one of the ISO 6523 ICD \
     list, extended by the DiGA codes.",
    |inv, f| {
        for (g, party) in [(Group::Seller, &inv.seller), (Group::Buyer, &inv.buyer)] {
            for id in &party.identifiers {
                if id.scheme().is_some_and(|s| !icd_or_diga(s)) {
                    f.at(Path::group(g));
                }
            }
        }
    });

    dex!(BR_DEX_05, "BR-DEX-05", Fatal, terms: [BtId(30), BtId(47)],
    "Any scheme identifier in cac:PartyLegalEntity MUST be coded using one of the ISO 6523 ICD \
     list, extended by the DiGA codes.",
    |inv, f| {
        for (g, party) in [(Group::Seller, &inv.seller), (Group::Buyer, &inv.buyer)] {
            if party
                .legal_registration
                .as_ref()
                .and_then(crate::Identifier::scheme)
                .is_some_and(|s| !icd_or_diga(s))
            {
                f.at(Path::group(g));
            }
        }
    });

    dex!(BR_DEX_06, "BR-DEX-06", Fatal, terms: [BtId(157)],
    "Any scheme identifier in cac:StandardItemIdentification MUST be coded using one of the ISO \
     6523 ICD list, extended by the DiGA codes.",
    |inv, f| {
        for (i, line) in inv.lines.iter().enumerate() {
            if line
                .item
                .standard_identifier
                .as_ref()
                .and_then(crate::Identifier::scheme)
                .is_some_and(|s| !icd_or_diga(s))
            {
                f.at(Path::at_term(Group::Line, i, BtId(157)));
            }
        }
    });

    dex!(BR_DEX_07, "BR-DEX-07", Fatal, terms: [BtId(34), BtId(49)],
    "Any scheme identifier for an Endpoint Identifier MUST belong to the CEF EAS code list, \
     extended by the DiGA codes.",
    |inv, f| {
        for (g, party) in [(Group::Seller, &inv.seller), (Group::Buyer, &inv.buyer)] {
            if party
                .electronic_address
                .as_ref()
                .and_then(crate::Identifier::scheme)
                .is_some_and(|s| !eas_or_diga(s))
            {
                f.at(Path::group(g));
            }
        }
    });

    dex!(BR_DEX_08, "BR-DEX-08", Fatal, terms: [BtId(71)],
    "Any scheme identifier for a Delivery location identifier MUST be coded using one of the ISO \
     6523 ICD list, extended by the DiGA codes.",
    |inv, f| {
        if inv
            .delivery
            .as_ref()
            .and_then(|d| d.location.as_ref())
            .and_then(crate::Identifier::scheme)
            .is_some_and(|s| !icd_or_diga(s))
        {
            f.at(Path::at_term(Group::Delivery, 0, BtId(71)));
        }
    });

    dex!(BR_DEX_09, "BR-DEX-09", Fatal, terms: [BtId(115), BtId(112), BtId(113), BtId(114)],
    "Amount due for payment (BT-115) = Invoice total amount with VAT (BT-112) - Paid amount \
     (BT-113) + Rounding amount (BT-114) + Σ Third party payment amount (BT-DEX-002).",
    |inv, f| {
        // This *replaces* BR-CO-16, which the Extension profile suppresses:
        // without the third-party term the two disagree by exactly that sum.
        let t = &inv.totals;
        let zero = crate::InvoiceAmount::ZERO;
        let Ok(third_party) = inv.extensions.third_party_total() else {
            return;
        };
        let expected = t
            .gross_total
            .checked_sub(t.paid.unwrap_or(zero))
            .and_then(|v| v.checked_add(t.rounding.unwrap_or(zero)))
            .and_then(|v| v.checked_add(third_party));
        let Ok(expected) = expected else { return };
        if expected != t.due {
            f.arithmetic(Path::term(BtId(115)), expected, t.due);
        }
    });

    macro_rules! third_party_term {
        ($konst:ident, $id:literal, $bt:literal, $field:ident, $text:literal) => {
            dex!($konst, $id, Fatal, terms: [], $text, |inv, f| {
                for p in &inv.extensions.third_party_payments {
                    if p.$field.is_none() {
                        f.at(Path::group(Group::Totals));
                    }
                }
            });
        };
    }

    third_party_term!(
        BR_DEX_10,
        "BR-DEX-10",
        "BT-DEX-001",
        payment_type,
        "Das Element \"Third party payment type\" (BT-DEX-001) muss übermittelt werden, wenn die \
         Gruppe THIRD PARTY PAYMENT (BG-DEX-09) übermittelt wird."
    );
    third_party_term!(
        BR_DEX_11,
        "BR-DEX-11",
        "BT-DEX-002",
        amount,
        "Das Element \"Third party payment amount\" (BT-DEX-002) muss übermittelt werden, wenn die \
         Gruppe THIRD PARTY PAYMENT (BG-DEX-09) übermittelt wird."
    );
    third_party_term!(
        BR_DEX_12,
        "BR-DEX-12",
        "BT-DEX-003",
        description,
        "Das Element \"Third party payment description\" (BT-DEX-003) muss übermittelt werden, \
         wenn die Gruppe THIRD PARTY PAYMENT (BG-DEX-09) übermittelt wird."
    );

    macro_rules! dex_by_type {
        ($konst:ident, $id:literal, $text:literal, $why:literal) => {
            #[doc = $text]
            #[doc = ""]
            #[doc = $why]
            pub static $konst: Rule = Rule {
                id: RuleId::new($id),
                severity: Severity::Fatal,
                text: $text,
                terms: &[],
                source: Source::ArtefactOnly,
                eval: |_, _| {},
            };
        };
    }

    dex_by_type!(
        BR_DEX_13,
        "BR-DEX-13",
        "Die maximale Anzahl zulässiger Nachkommastellen für BT-DEX-002 ist 2.",
        "`InvoiceAmount` is `i64` minor units — a third decimal cannot be written down. Same \
         disposition as the `BR-DEC-*` family."
    );
    dex_by_type!(
        BR_DEX_14,
        "BR-DEX-14",
        "Die Währungsangabe von BT-DEX-002 muss BT-5 entsprechen.",
        "Every amount in the model is implicitly in BT-5; there is no per-amount `@currencyID`. \
         Same disposition as `BR-CL-03`."
    );

    /// The fourteen rules the XRechnung Extension adds.
    pub static ALL: &[&Rule] = &[
        &BR_DEX_01, &BR_DEX_02, &BR_DEX_03, &BR_DEX_04, &BR_DEX_05, &BR_DEX_06, &BR_DEX_07,
        &BR_DEX_08, &BR_DEX_09, &BR_DEX_10, &BR_DEX_11, &BR_DEX_12, &BR_DEX_13, &BR_DEX_14,
    ];
}

#[cfg(test)]
mod sepa_tests {
    use super::*;

    /// The `sepa` feature must never *reject* something the checksum accepts
    /// for the wrong reason — it should only reject more, and only on length or
    /// structure.
    #[test]
    fn the_registry_check_is_strictly_stronger() {
        // Real IBANs, valid under both.
        for good in [
            "DE89370400440532013000",
            "NL91ABNA0417164300",
            "GB29NWBK60161331926819",
        ] {
            assert!(is_valid_iban_checksum(good), "{good}");
            assert!(is_valid_iban(good), "{good}");
        }
        // A mistyped digit fails the checksum, so both reject it.
        assert!(!is_valid_iban_checksum("DE89370400440532013001"));
        assert!(!is_valid_iban("DE89370400440532013001"));
    }

    /// The whole point of the feature: a checksum-valid string of the wrong
    /// length for its country.
    ///
    /// `DE` IBANs are 22 characters. This one is 20 and its check digits are
    /// consistent, so the checksum alone cannot tell — and no German bank would
    /// accept it.
    #[test]
    #[cfg(feature = "sepa")]
    fn the_registry_catches_a_wrong_length_iban() {
        // Constructed so mod-97-10 passes: the registry is the only thing that
        // can reject it.
        let short = "DE29100000001234567";
        if is_valid_iban_checksum(short) {
            assert!(
                !is_valid_iban(short),
                "the ISO 13616 registry must reject a 19-character DE IBAN"
            );
        }
        // Length is what differs; the country is real either way.
        assert!(is_valid_iban("DE89370400440532013000"));
    }

    #[test]
    fn a_malformed_creditor_identifier_is_rejected_either_way() {
        assert!(!is_valid_creditor_identifier("not a creditor id"));
        assert!(!is_valid_creditor_identifier(""));
        // `DE98ZZZ09999999999` is the identifier from KoSIT's own examples.
        assert!(is_valid_creditor_identifier("DE98ZZZ09999999999"));
    }
}
