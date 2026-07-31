//! Read an inbound document and validate it — the receiving path, end to end.
//!
//! ```sh
//! cargo run --example read_and_validate --features cii
//! ```
//!
//! This is the whole point of the two-crate split: this crate turns bytes into
//! an `Invoice`, and `en16931` decides whether that invoice is correct. Neither
//! knows anything about the other's job.
//!
//! Note what the reader hands back besides the model. **`unmapped`** names every
//! element it saw and did not map, and **`malformed`** every value that was
//! present but not representable. A reader that returned only the invoice would
//! let a document with six ignored elements come back looking clean.

use en16931_formats::{Syntax, sniff, ubl};

/// A small but complete UBL invoice.
const DOCUMENT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"
         xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
         xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
  <cbc:CustomizationID>urn:cen.eu:en16931:2017</cbc:CustomizationID>
  <cbc:ID>RE-2026-0042</cbc:ID>
  <cbc:IssueDate>2026-01-15</cbc:IssueDate>
  <cbc:DueDate>2026-02-15</cbc:DueDate>
  <cbc:InvoiceTypeCode>380</cbc:InvoiceTypeCode>
  <cbc:DocumentCurrencyCode>EUR</cbc:DocumentCurrencyCode>
  <cac:AccountingSupplierParty><cac:Party>
    <cac:PostalAddress><cbc:CityName>M&#252;nchen</cbc:CityName>
      <cac:Country><cbc:IdentificationCode>DE</cbc:IdentificationCode></cac:Country>
    </cac:PostalAddress>
    <cac:PartyTaxScheme><cbc:CompanyID>DE123456789</cbc:CompanyID>
      <cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme></cac:PartyTaxScheme>
    <cac:PartyLegalEntity><cbc:RegistrationName>Seller GmbH</cbc:RegistrationName></cac:PartyLegalEntity>
  </cac:Party></cac:AccountingSupplierParty>
  <cac:AccountingCustomerParty><cac:Party>
    <cac:PostalAddress><cbc:CityName>Hamburg</cbc:CityName>
      <cac:Country><cbc:IdentificationCode>DE</cbc:IdentificationCode></cac:Country>
    </cac:PostalAddress>
    <cac:PartyLegalEntity><cbc:RegistrationName>Buyer AG</cbc:RegistrationName></cac:PartyLegalEntity>
  </cac:Party></cac:AccountingCustomerParty>
  <cac:TaxTotal>
    <cbc:TaxAmount currencyID="EUR">38.00</cbc:TaxAmount>
    <cac:TaxSubtotal>
      <cbc:TaxableAmount currencyID="EUR">200.00</cbc:TaxableAmount>
      <cbc:TaxAmount currencyID="EUR">38.00</cbc:TaxAmount>
      <cac:TaxCategory><cbc:ID>S</cbc:ID><cbc:Percent>19</cbc:Percent>
        <cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme></cac:TaxCategory>
    </cac:TaxSubtotal>
  </cac:TaxTotal>
  <cac:LegalMonetaryTotal>
    <cbc:LineExtensionAmount currencyID="EUR">200.00</cbc:LineExtensionAmount>
    <cbc:TaxExclusiveAmount currencyID="EUR">200.00</cbc:TaxExclusiveAmount>
    <cbc:TaxInclusiveAmount currencyID="EUR">238.00</cbc:TaxInclusiveAmount>
    <cbc:PayableAmount currencyID="EUR">238.00</cbc:PayableAmount>
  </cac:LegalMonetaryTotal>
  <cac:InvoiceLine>
    <cbc:ID>1</cbc:ID>
    <cbc:InvoicedQuantity unitCode="C62">2</cbc:InvoicedQuantity>
    <cbc:LineExtensionAmount currencyID="EUR">200.00</cbc:LineExtensionAmount>
    <cac:Item><cbc:Name>Widget</cbc:Name>
      <cac:ClassifiedTaxCategory><cbc:ID>S</cbc:ID><cbc:Percent>19</cbc:Percent>
        <cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme></cac:ClassifiedTaxCategory>
    </cac:Item>
    <cac:Price><cbc:PriceAmount currencyID="EUR">100</cbc:PriceAmount></cac:Price>
  </cac:InvoiceLine>
</Invoice>"#;

fn main() {
    // Which syntax is this? Cheap, and it refuses to guess when the answer is
    // neither — "not an e-invoice" and "an e-invoice in the other syntax" need
    // different messages.
    println!("syntax: {:?}", sniff(DOCUMENT));
    assert_eq!(sniff(DOCUMENT), Some(Syntax::Ubl));

    let read = ubl::from_str(DOCUMENT).expect("a well-formed UBL invoice");

    println!("unmapped elements: {:?}", read.unmapped);
    println!("malformed values:  {:?}\n", read.malformed);

    let invoice = read.invoice;
    println!("BT-1  invoice number: {:?}", invoice.number);
    println!("BT-2  issue date:     {:?}", invoice.issue_date);
    println!("BT-27 seller name:    {:?}", invoice.seller.name);
    println!("BT-112 gross total:   {}", invoice.totals.gross_total);
    println!("lines:                {}\n", invoice.lines.len());

    // `en16931` decides correctness. This crate re-implements none of it.
    let report = en16931::validate(&invoice);
    println!(
        "valid: {} ({} rules)",
        report.is_valid(),
        report.rules_checked()
    );
    for f in report.findings() {
        println!(
            "  [{}] {} — {}\n      {}",
            f.severity, f.rule, f.path, f.message
        );
    }
}
